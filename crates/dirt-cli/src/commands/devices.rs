//! `dirt devices`: the account's devices, approving one that is waiting, and
//! removing one.
//!
//! Approving runs the same exchange as the desktop and Android apps, in the
//! terminal: both devices show a code, and the person says whether the two
//! are the same.

use std::io::{self, BufRead, Write};
use std::time::Duration;

use dirt_core::api::{DeviceInfo, DeviceStatus};
use dirt_core::enrolment::CancelReason;
use dirt_core::session::{ExchangeStep, Session};
use dirt_core::util::as_sentence;

use crate::cli::DeviceCommands;
use crate::commands::common::format_relative_time;
use crate::context::Context;
use crate::error::CliError;

/// The pause between two looks at an exchange.
const LOOK_EVERY: Duration = Duration::from_secs(2);
/// How long an exchange waits for the other device before giving up.
const GIVE_UP_AFTER: Duration = Duration::from_secs(300);

pub async fn run_devices(
    command: Option<DeviceCommands>,
    context: &Context,
) -> Result<(), CliError> {
    let session = context.require_session()?;
    match command {
        None | Some(DeviceCommands::List) => list(session).await,
        Some(DeviceCommands::Approve { device }) => approve(session, device.as_deref()).await,
        Some(DeviceCommands::Remove { device }) => remove(session, &device).await,
    }
}

fn failed(error: impl std::fmt::Display) -> CliError {
    CliError::Account(as_sentence(&error))
}

fn describe(device: &DeviceInfo, now_ms: i64) -> String {
    let status = match device.status {
        DeviceStatus::Approved => "approved",
        DeviceStatus::Pending => "waiting to be approved",
        DeviceStatus::Revoked => "removed",
    };
    let this = if device.current {
        "  (this device)"
    } else {
        ""
    };
    format!(
        "{}  {:<24}  {status}, last seen {}{this}",
        &device.id.to_string()[..13],
        device.name,
        format_relative_time(device.last_seen_ms, now_ms),
    )
}

async fn list(session: &Session) -> Result<(), CliError> {
    let devices = session.devices().await.map_err(failed)?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    for device in &devices {
        println!("{}", describe(device, now_ms));
    }
    if devices
        .iter()
        .any(|device| device.status == DeviceStatus::Pending)
    {
        println!("Run `dirt devices approve` to let a waiting device in.");
    }
    Ok(())
}

/// Finds the one device that `prefix` names, among `devices`.
fn pick<'a>(devices: &'a [DeviceInfo], prefix: &str) -> Result<&'a DeviceInfo, CliError> {
    let matching: Vec<&DeviceInfo> = devices
        .iter()
        .filter(|device| device.id.to_string().starts_with(prefix))
        .collect();
    match matching.as_slice() {
        [one] => Ok(one),
        [] => Err(CliError::Account(format!(
            "No device has an ID starting with {prefix}. Run `dirt devices` to see them."
        ))),
        _ => Err(CliError::Account(format!(
            "More than one device has an ID starting with {prefix}. Give more of it."
        ))),
    }
}

async fn remove(session: &Session, prefix: &str) -> Result<(), CliError> {
    let devices = session.devices().await.map_err(failed)?;
    let device = pick(&devices, prefix)?;
    if device.current {
        return Err(CliError::Account(
            "That is this device. Use `dirt auth logout` to sign it out.".to_owned(),
        ));
    }
    session.remove_device(device.id).await.map_err(failed)?;
    println!(
        "Removed {}. It can no longer reach the server.",
        device.name
    );
    println!(
        "It keeps the notes it already had and the key that reads them. \
         Changing the account's key, which ends that, is not available yet."
    );
    Ok(())
}

/// Reads one line of the person's answer. Works when the answer is piped
/// in, too; no answer at all counts as "no".
pub fn ask(question: &str) -> Result<bool, CliError> {
    print!("{question}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// What the person is told when the two codes differ.
pub const MISMATCH_WARNING: &str = "The codes did not match. The device was not let in. \
     Someone may be interfering with the connection between your devices; \
     try again on a network you trust.";

async fn approve(session: &Session, prefix: Option<&str>) -> Result<(), CliError> {
    let waiting = session.waiting_devices().await.map_err(failed)?;
    let device = match (prefix, waiting.as_slice()) {
        (Some(prefix), _) => pick(&waiting, prefix)?.clone(),
        (None, [one]) => one.clone(),
        (None, []) => {
            println!("No device is waiting to be approved.");
            return Ok(());
        }
        (None, _) => {
            let now_ms = chrono::Utc::now().timestamp_millis();
            for device in &waiting {
                println!("{}", describe(device, now_ms));
            }
            return Err(CliError::Account(
                "More than one device is waiting. Say which: `dirt devices approve <ID>`."
                    .to_owned(),
            ));
        }
    };
    println!(
        "Approving {}. Open Dirt on that device and choose to be approved by another device.",
        device.name
    );

    let mut approval = session.begin_approval(device).map_err(failed)?;
    // When the exchange last moved. With no movement for long enough, the
    // command gives up, whatever it was waiting for.
    let mut last_progress = std::time::Instant::now();
    // The code the person has answered for. The other device may begin
    // again, which gives a new code that has to be compared afresh.
    let mut answered_for: Option<String> = None;
    loop {
        match approval.poll().await.map_err(failed)? {
            ExchangeStep::Compare { code } if answered_for.as_deref() != Some(code.as_str()) => {
                if answered_for.is_some() {
                    println!("The other device began again. There is a new code.");
                }
                println!("Code: {code}");
                if ask("Does the other device show the same code? [y/N] ")? {
                    approval.confirm().await.map_err(failed)?;
                    println!("Waiting for the other device to confirm too.");
                } else {
                    approval.reject(CancelReason::Mismatch).await;
                    return Err(CliError::Account(MISMATCH_WARNING.to_owned()));
                }
                answered_for = Some(code);
                last_progress = std::time::Instant::now();
                continue;
            }
            ExchangeStep::Waiting | ExchangeStep::Finishing | ExchangeStep::Compare { .. } => {
                if last_progress.elapsed() > GIVE_UP_AFTER {
                    return Err(CliError::Account(
                        "The other device did not answer. The device was not let in; try again."
                            .to_owned(),
                    ));
                }
            }
            ExchangeStep::Done => {
                println!("Approved. That device can now read and write the account's notes.");
                return Ok(());
            }
            ExchangeStep::Cancelled(CancelReason::Mismatch) => {
                return Err(CliError::Account(MISMATCH_WARNING.to_owned()));
            }
            ExchangeStep::Cancelled(CancelReason::Declined) => {
                return Err(CliError::Account(
                    "The other device called it off. It was not let in.".to_owned(),
                ));
            }
            ExchangeStep::HandledElsewhere => {
                println!("Another of your devices is already dealing with that device.");
                return Ok(());
            }
        }
        tokio::time::sleep(LOOK_EVERY).await;
    }
}

/// On a waiting device: asks to be let in by another device, and goes
/// through the exchange with it.
pub async fn join(session: &Session) -> Result<(), CliError> {
    let mut join = session.begin_join().await.map_err(failed)?;
    println!(
        "On a device that is already signed in, open Dirt and approve this device, \
         or run `dirt devices approve` there."
    );
    let mut last_progress = std::time::Instant::now();
    let mut answered_for: Option<String> = None;
    loop {
        match join.poll().await.map_err(failed)? {
            ExchangeStep::Compare { code } if answered_for.as_deref() != Some(code.as_str()) => {
                println!("Code: {code}");
                if ask("Does the other device show the same code? [y/N] ")? {
                    join.confirm().await.map_err(failed)?;
                    println!("Waiting for the other device to confirm too.");
                } else {
                    join.reject(CancelReason::Mismatch).await.map_err(failed)?;
                    return Err(CliError::Account(MISMATCH_WARNING.to_owned()));
                }
                answered_for = Some(code);
                last_progress = std::time::Instant::now();
                continue;
            }
            ExchangeStep::Waiting
            | ExchangeStep::Finishing
            | ExchangeStep::HandledElsewhere
            | ExchangeStep::Compare { .. } => {
                if last_progress.elapsed() > GIVE_UP_AFTER {
                    return Err(CliError::Account(
                        "No device answered. This device is still waiting; try again.".to_owned(),
                    ));
                }
            }
            ExchangeStep::Done => {
                println!("This device was approved. Run `dirt sync` to fetch the notes.");
                return Ok(());
            }
            ExchangeStep::Cancelled(CancelReason::Mismatch) => {
                return Err(CliError::Account(MISMATCH_WARNING.to_owned()));
            }
            ExchangeStep::Cancelled(CancelReason::Declined) => {
                return Err(CliError::Account(
                    "The other device did not let this one in. It has been signed out.".to_owned(),
                ));
            }
        }
        tokio::time::sleep(LOOK_EVERY).await;
    }
}
