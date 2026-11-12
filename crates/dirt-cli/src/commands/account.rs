//! `dirt auth`: signing in and out, and what the account's state is.

use std::io::{self, BufRead, IsTerminal, Write};

use dirt_core::session::{Session, SessionState, SignOutError, Unsynced};

use crate::cli::AuthCommands;
use crate::context::{describe, Context};
use crate::error::CliError;

pub async fn run_auth(command: AuthCommands, context: &Context) -> Result<(), CliError> {
    match command {
        AuthCommands::Status => {
            for line in status_lines(context).await? {
                println!("{line}");
            }
            Ok(())
        }
        AuthCommands::Login { move_local_notes } => {
            login(context.require_session()?, move_local_notes).await
        }
        AuthCommands::Logout { discard_unsynced } => {
            logout(context.require_session()?, discard_unsynced).await
        }
    }
}

fn account_error(error: impl std::fmt::Display) -> CliError {
    CliError::Account(error.to_string())
}

/// What `dirt auth status` prints.
pub async fn status_lines(context: &Context) -> Result<Vec<String>, CliError> {
    let Some(session) = &context.session else {
        return Ok(vec![
            "No server is configured. Notes are kept on this device only.".to_owned(),
        ]);
    };
    let mut lines = Vec::new();
    match session.state() {
        SessionState::SignedOut | SessionState::SigningIn => {
            lines.push("Not signed in. Notes are kept on this device only.".to_owned());
            lines.push("Run `dirt auth login` to sign in.".to_owned());
        }
        SessionState::WaitingForApproval { account_id, .. } => {
            lines.push(format!("Signed in to account {account_id}."));
            lines.push(
                "This device is waiting to be approved by another device of the account, \
                 and cannot read its notes yet."
                    .to_owned(),
            );
        }
        SessionState::Ready { account_id, .. } => {
            lines.push(format!("Signed in to account {account_id}."));
            let store = session.store();
            let last = store.last_synced_ms().await?;
            lines.push(last.map_or_else(
                || "Never synced.".to_owned(),
                |at| {
                    format!(
                        "Last synced {}.",
                        crate::commands::common::format_relative_time(
                            at,
                            chrono::Utc::now().timestamp_millis()
                        )
                    )
                },
            ));
            let unsent = store.unsent_count().await?;
            if unsent > 0 {
                lines.push(format!("{unsent} note(s) waiting to be sent."));
            }
            for problem in store.note_problems().await? {
                lines.push(format!(
                    "Note {} is not syncing: {}.",
                    problem.note_id, problem.reason
                ));
            }
            let waiting = session.local_notes_waiting().await.map_err(account_error)?;
            if waiting > 0 {
                lines.push(format!(
                    "{waiting} note(s) written before signing in are not in the account. \
                     Run `dirt auth login --move-local-notes` to move them."
                ));
            }
        }
    }
    Ok(lines)
}

async fn login(session: &Session, move_local_notes: bool) -> Result<(), CliError> {
    // A device that is ready signs in again only when the server has told
    // it to; otherwise there is nothing to do but move notes if asked.
    let must_sign_in = !session.state().is_ready() || needs_sign_in_again(session).await;
    if must_sign_in {
        let state = session
            .sign_in_with_browser(|address| {
                println!("Opening your browser to sign in. If it does not open, go to:");
                println!("{address}");
                println!("Waiting for you to finish in the browser. Press Ctrl+C to cancel.");
                // The address is printed above, so a browser that cannot be
                // opened is not a failure.
                let _ = webbrowser::open(address);
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
        Err(SignOutError::Unsynced { notes, cause }) => {
            let why = cause.map_or_else(String::new, |cause| format!(" ({cause})"));
            Err(CliError::Account(format!(
                "{notes} note(s) have not reached the server{why} and signing out would delete \
                 them. Save them with `dirt export`, then run \
                 `dirt auth logout --discard-unsynced`"
            )))
        }
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
