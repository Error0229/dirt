//! `dirt auth`: signing in and out, and what the account's state is.

use std::io::{self, BufRead, IsTerminal, Write};

use dirt_core::config::AddressSource;
use dirt_core::session::{KeyChange, ResetOutcome, Session, SessionState, SignOutError, Unsynced};
use dirt_core::util::as_sentence;

use crate::cli::{AuthCommands, PhraseCommands};
use crate::context::{describe, Context};
use crate::error::CliError;

pub async fn run_auth(command: AuthCommands, context: Context) -> Result<(), CliError> {
    // The two commands that remove the account's store from this computer
    // let go of the store first, so that its files can go at once and not
    // only the next time `dirt` runs.
    let (command, context) = match command {
        AuthCommands::Logout { discard_unsynced } => {
            let session = context.into_session()?;
            logout(&session, discard_unsynced).await?;
            session.remove_signed_out_stores();
            return Ok(());
        }
        AuthCommands::DeleteAccount { yes } => {
            let session = context.into_session()?;
            delete_account(&session, yes).await?;
            session.remove_signed_out_stores();
            return Ok(());
        }
        other => (other, &context),
    };
    match command {
        AuthCommands::Status => {
            for line in status_lines(context).await? {
                println!("{line}");
            }
            Ok(())
        }
        AuthCommands::Login {
            move_local_notes,
            no_browser,
        } => login(context.require_session()?, move_local_notes, no_browser).await,
        AuthCommands::Logout { .. } | AuthCommands::DeleteAccount { .. } => {
            unreachable!("handled above")
        }
        AuthCommands::Join => crate::commands::devices::join(context.require_session()?).await,
        AuthCommands::Recover => recover(context.require_session()?).await,
        AuthCommands::Phrase { command } => phrase(context.require_session()?, command).await,
        AuthCommands::Reset { yes } => reset(context.require_session()?, yes).await,
        AuthCommands::ChangeKey { yes } => change_key(context.require_session()?, yes).await,
        AuthCommands::CancelReset => {
            context
                .require_session()?
                .cancel_reset()
                .await
                .map_err(sentence)?;
            println!("The reset was refused. Nothing was changed.");
            Ok(())
        }
    }
}

fn sentence(error: impl std::fmt::Display) -> CliError {
    CliError::Account(as_sentence(&error))
}

fn when(at_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at_ms).map_or_else(
        || "later".to_owned(),
        |at| at.format("%Y-%m-%d %H:%M UTC").to_string(),
    )
}

/// On a waiting device: opens the account with its recovery phrase.
async fn recover(session: &Session) -> Result<(), CliError> {
    if session.state().is_ready() {
        println!("This device already reads the account's notes.");
        return Ok(());
    }
    println!("Type the account's recovery phrase, all 24 words, then press Enter:");
    let mut typed = String::new();
    io::stdin().lock().read_line(&mut typed)?;
    session
        .recover_with_phrase(&typed)
        .await
        .map_err(sentence)?;
    println!("This device was let in. Run `dirt sync` to fetch the notes.");
    Ok(())
}

async fn phrase(session: &Session, command: PhraseCommands) -> Result<(), CliError> {
    match command {
        PhraseCommands::New => {
            let phrase = session.create_recovery_phrase().await.map_err(sentence)?;
            println!("The account's recovery phrase:");
            println!();
            for (index, line) in phrase
                .words()
                .split(' ')
                .collect::<Vec<_>>()
                .chunks(6)
                .enumerate()
            {
                println!("  {:>2}.  {}", index * 6 + 1, line.join(" "));
            }
            println!();
            println!(
                "Write it down and keep it somewhere safe. It is not stored anywhere and will not be shown again."
            );
            println!(
                "Anyone who has it and can sign in as you can read your notes. A phrase made earlier no longer works."
            );
        }
        PhraseCommands::Remove => {
            session.remove_recovery_phrase().await.map_err(sentence)?;
            println!("The recovery phrase was removed. It no longer opens the account.");
        }
    }
    Ok(())
}

/// Changes the account's key and says what follows from it.
pub async fn change_key(session: &Session, yes: bool) -> Result<(), CliError> {
    let question = "Changing the account's key means that a device removed earlier can read \
                    nothing written from now on. The recovery phrase stops working and has \
                    to be made again. Change the key? [y/N] ";
    if !yes && !confirm(question)? {
        println!("Nothing was changed.");
        return Ok(());
    }
    // Asked before the change: afterwards there is none either way.
    let had_phrase = session
        .my_device()
        .await
        .is_ok_and(|me| me.recovery_available);
    match session.change_account_key().await.map_err(sentence)? {
        KeyChange::Changed { old_key_notes, .. } => {
            println!("The account's key was changed.");
            match old_key_notes {
                0 => println!("Every note is under the new key."),
                1 => println!(
                    "1 note is still under the old key. It is sent again as your devices sync."
                ),
                count => println!(
                    "{count} notes are still under the old key. They are sent again as your \
                     devices sync."
                ),
            }
        }
        KeyChange::ChangedByAnotherDevice { .. } => {
            println!(
                "Another device changed the account's key just now. This device has the new key."
            );
        }
    }
    if had_phrase {
        println!("The recovery phrase no longer works. `dirt auth phrase new` makes a new one.");
    }
    Ok(())
}

async fn reset(session: &Session, yes: bool) -> Result<(), CliError> {
    let question = "Resetting deletes the account's notes on the server, its recovery phrase \
                    and every other device. Notes on this device are kept and sent again. \
                    Reset the account? [y/N] ";
    if !yes && !confirm(question)? {
        println!("Nothing was changed.");
        return Ok(());
    }
    match session.reset_account().await.map_err(sentence)? {
        ResetOutcome::Done => {
            println!("The account was reset. This is now its only device, with a new key.");
            match session.sync_now().await {
                Ok(report) => println!("Synced: {} sent.", report.pushed),
                Err(error) => eprintln!("Not synced yet: {}", describe(&error)),
            }
        }
        ResetOutcome::Scheduled { effective_at_ms } => {
            println!(
                "This device is not approved, and the account holds notes, so the reset \
                 has to wait. The account's other devices are told, and any of them can \
                 refuse it."
            );
            println!(
                "Run `dirt auth reset` again after {} to go ahead.",
                when(effective_at_ms)
            );
        }
    }
    Ok(())
}

async fn delete_account(session: &Session, yes: bool) -> Result<(), CliError> {
    let question = "Deleting removes the account and everything the server holds for it, \
                    for every device. The notes on this computer are kept, outside any \
                    account. Delete the account? [y/N] ";
    if !yes && !confirm(question)? {
        println!("Nothing was changed.");
        return Ok(());
    }
    session.delete_account().await.map_err(sentence)?;
    println!("The account was deleted. Its notes are still on this computer.");
    Ok(())
}

fn account_error(error: impl std::fmt::Display) -> CliError {
    CliError::Account(error.to_string())
}

/// What `dirt auth status` prints.
pub async fn status_lines(context: &Context) -> Result<Vec<String>, CliError> {
    let Some(session) = &context.session else {
        return Ok(vec![
            "No server is configured. Notes are kept on this device only.".to_owned(),
            "Set DIRT_API_BASE_URL to a server's address to sync.".to_owned(),
        ]);
    };
    let mut lines = Vec::new();
    if let Some(server) = &context.server {
        let source = match server.source {
            AddressSource::Environment => "from DIRT_API_BASE_URL",
            AddressSource::Build => "the address this program was built with",
        };
        lines.push(format!("Server: {} ({source}).", server.url));
    }
    match session.state() {
        SessionState::SignedOut | SessionState::SigningIn => {
            lines.push("Not signed in. Notes are kept on this device only.".to_owned());
            lines.push("Run `dirt auth login` to sign in.".to_owned());
        }
        SessionState::WaitingForApproval { account_id, .. } => {
            lines.push(format!("Signed in to account {account_id}."));
            lines.push(
                "This device is waiting to be let in, and cannot read the notes yet.".to_owned(),
            );
            // What the person can do depends on what the account has.
            match session.my_device().await {
                Ok(me) => {
                    if me.approvers > 0 {
                        lines.push(
                            "Run `dirt auth join` here, and approve it on a device that is \
                             already signed in."
                                .to_owned(),
                        );
                    }
                    if me.recovery_available {
                        lines.push(
                            "Or run `dirt auth recover` and type the recovery phrase.".to_owned(),
                        );
                    }
                    if me.approvers == 0 && !me.recovery_available {
                        lines.push(
                            "No device is left that could let it in, and the account has no \
                             recovery phrase. `dirt auth reset` starts the account again."
                                .to_owned(),
                        );
                    }
                    if let Some(at) = me.reset_effective_at_ms {
                        lines.push(format!(
                            "A reset was asked for. It can go ahead after {}.",
                            when(at)
                        ));
                    }
                }
                Err(error) => lines.push(format!("The server could not be asked: {error}.")),
            }
        }
        SessionState::Ready { account_id, .. } => {
            lines.push(format!("Signed in to account {account_id}."));
            // The same view the apps show and `dirt sync --verbose` prints.
            let overview = session.sync_overview().await.map_err(account_error)?;
            lines.extend(overview.lines(dirt_core::util::now_ms()));
            let waiting = session.local_notes_waiting().await.map_err(account_error)?;
            if waiting > 0 {
                lines.push(format!(
                    "{waiting} note(s) written before signing in are not in the account. \
                     Run `dirt auth login --move-local-notes` to move them."
                ));
            }
            // What needs the person's attention. The server is asked; with
            // no network these lines are left out.
            if let Ok(me) = session.my_device().await {
                if !me.recovery_available && me.approvers == 0 {
                    lines.push(
                        "This is the only device that can read the account, and it has no \
                         recovery phrase. `dirt auth phrase new` makes one."
                            .to_owned(),
                    );
                }
                if let Some(at) = me.reset_effective_at_ms {
                    lines.push(format!(
                        "A device that is not approved asked to reset this account. It takes \
                         effect after {}. If that was not you, run `dirt auth cancel-reset`.",
                        when(at)
                    ));
                }
            }
            if let Ok(devices) = session.waiting_devices().await {
                if !devices.is_empty() {
                    lines.push(format!(
                        "{} device(s) are waiting to be let in. Run `dirt devices approve`.",
                        devices.len()
                    ));
                }
            }
        }
    }
    Ok(lines)
}

async fn login(
    session: &Session,
    move_local_notes: bool,
    no_browser: bool,
) -> Result<(), CliError> {
    // A device that is ready signs in again only when the server has told
    // it to; otherwise there is nothing to do but move notes if asked.
    let must_sign_in = !session.state().is_ready() || needs_sign_in_again(session).await;
    if must_sign_in {
        let state = session
            .sign_in_with_browser(|address| {
                if no_browser {
                    println!("Open this address in a browser on this computer to sign in:");
                } else {
                    println!("Opening your browser to sign in. If it does not open, go to:");
                }
                println!("{address}");
                println!("Waiting for you to finish in the browser. Press Ctrl+C to cancel.");
                if !no_browser {
                    // The address is printed above, so a browser that cannot
                    // be opened is not a failure.
                    let _ = webbrowser::open(address);
                }
                Ok(())
            })
            .await
            .map_err(|error| CliError::Account(describe(&error)))?;
        match state {
            SessionState::Ready { account_id, .. } => {
                println!("Signed in to account {account_id}.");
            }
            SessionState::WaitingForApproval { .. } => {
                println!(
                    "Signed in. This device must be approved by another device of the \
                     account before it can read its notes."
                );
                return Ok(());
            }
            SessionState::SignedOut | SessionState::SigningIn => {
                return Err(CliError::Account("sign-in did not complete".to_owned()));
            }
        }
    } else {
        println!("Already signed in.");
    }

    let waiting = session.local_notes_waiting().await.map_err(account_error)?;
    if waiting > 0 {
        let question = format!(
            "{waiting} note(s) were written on this device before signing in. \
             Move them into the account? [y/N] "
        );
        if move_local_notes || confirm(&question)? {
            let moved = session.move_local_notes().await.map_err(account_error)?;
            println!("Moved {moved} note(s) into the account.");
        } else {
            println!(
                "They stay on this device, outside the account. \
                 Run `dirt auth login --move-local-notes` to move them later."
            );
        }
    }

    match session.sync_now().await {
        Ok(report) => println!(
            "Synced: {} received, {} sent.",
            report.pulled, report.pushed
        ),
        Err(error) => eprintln!("Signed in, but not synced: {}", describe(&error)),
    }
    Ok(())
}

/// Whether the server refuses this device's token, so that signing in again
/// is what is needed.
async fn needs_sign_in_again(session: &Session) -> bool {
    use dirt_core::api::ErrorClass;
    use dirt_core::session::SessionError;

    match session.sync_now().await {
        Err(SessionError::Sync(error)) => error.class() == ErrorClass::StopUntilSignedIn,
        _ => false,
    }
}

async fn logout(session: &Session, discard_unsynced: bool) -> Result<(), CliError> {
    if session.state() == SessionState::SignedOut {
        println!("Not signed in.");
        return Ok(());
    }
    let choice = if discard_unsynced {
        Unsynced::Discard
    } else {
        Unsynced::Refuse
    };
    match session.sign_out(choice).await {
        Ok(()) => {
            println!("Signed out. This account's notes and keys were removed from this device.");
            Ok(())
        }
        Err(SignOutError::Unsynced {
            notes,
            files,
            cause,
        }) => Err(CliError::Account(format!(
            "{} To keep a copy of the notes, run `dirt export`. To sign out all the same, \
             run `dirt auth logout --discard-unsynced`",
            dirt_core::sync::overview::sign_out_would_lose(notes, files, cause.as_deref(),)
        ))),
        Err(error) => Err(account_error(error)),
    }
}

/// Asks a yes-or-no question. Without a terminal to ask on, the answer is no.
fn confirm(question: &str) -> Result<bool, CliError> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    print!("{question}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}
