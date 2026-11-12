//! `dirt sync`: one sync cycle, now.

use dirt_core::session::SessionError;
use dirt_core::sync::worker::SyncFailure;

use crate::context::{describe, Context};
use crate::error::CliError;

/// Runs one cycle and says what it did. With `verbose`, also prints the
/// sync status view the apps show: when the last sync succeeded, what is
/// waiting, and each note that could not be synced with what releases it.
/// The view is printed whether or not the cycle worked, because a failure
/// is when it is wanted most.
pub async fn run_sync(context: &Context, verbose: bool) -> Result<(), CliError> {
    let session = context.require_session()?;
    let outcome = session.sync_now().await;

    if let Ok(report) = &outcome {
        println!(
            "Synced: {} received, {} sent.",
            report.pulled, report.pushed
        );
    }
    if verbose {
        if let Ok(mut overview) = session.sync_overview().await {
            if let Ok(report) = &outcome {
                overview.old_key_notes = report.old_key_notes;
            }
            if let Err(SessionError::Sync(error)) = &outcome {
                overview.failure = Some(SyncFailure {
                    class: error.class(),
                    message: error.to_string(),
                });
            }
            for line in overview.lines(dirt_core::util::now_ms()) {
                println!("{line}");
            }
        }
    } else if let Ok(report) = &outcome {
        if report.unsent > 0 {
            println!(
                "{} note(s) are still waiting; another device was changing them. Run `dirt sync` again.",
                report.unsent
            );
        }
        let problems = session.store().note_problems().await?;
        for problem in &problems {
            println!(
                "Not syncing: {} ({}).",
                problem.title(),
                dirt_core::util::as_sentence(&problem.reason).trim_end_matches('.')
            );
        }
        if !problems.is_empty() {
            println!("Run `dirt sync --verbose` for what releases each.");
        }
    }

    outcome
        .map(|_| ())
        .map_err(|error| CliError::Account(describe(&error)))
}
