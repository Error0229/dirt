//! `dirt sync`: one sync cycle, now.

use dirt_core::session::SessionError;
use dirt_core::store::FileProblem;
use dirt_core::sync::engine::SyncReport;
use dirt_core::sync::overview::SyncOverview;
use dirt_core::sync::worker::SyncFailure;

use crate::context::{describe, Context};
use crate::error::CliError;

/// What a cycle did, in one line. Files are mentioned when any were sent.
pub fn synced_line(report: &SyncReport) -> String {
    let files = match report.files_sent {
        0 => String::new(),
        1 => ", and 1 file".to_owned(),
        count => format!(", and {count} files"),
    };
    format!(
        "Synced: {} received, {} sent{files}.",
        report.pulled, report.pushed
    )
}

/// What a cycle left unsent of files: how many wait, and each the server
/// refused with the reason. Nothing when every file got through. In the
/// words every client uses.
pub fn files_not_sent(report: &SyncReport, refused: &[FileProblem]) -> Vec<String> {
    let mut lines = Vec::new();
    let waiting = SyncOverview {
        files_unsent: report.files_unsent,
        ..SyncOverview::default()
    }
    .waiting();
    if let Some(waiting) = waiting {
        let why = if report.files_failed {
            " The file storage could not be reached; `dirt sync` tries again."
        } else {
            ""
        };
        lines.push(format!("{waiting}.{why}"));
    }
    for problem in refused {
        lines.push(format!(
            "Not sent: {} ({}).",
            problem.title(),
            dirt_core::util::as_sentence(&problem.reason).trim_end_matches('.')
        ));
    }
    lines
}

/// Runs one cycle and says what it did. With `verbose`, also prints the
/// sync status view the apps show: when the last sync succeeded, what is
/// waiting, and each note that could not be synced with what releases it.
/// The view is printed whether or not the cycle worked, because a failure
/// is when it is wanted most.
pub async fn run_sync(context: &Context, verbose: bool) -> Result<(), CliError> {
    let session = context.require_session()?;
    let outcome = session.sync_now().await;

    if let Ok(report) = &outcome {
        println!("{}", synced_line(report));
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
        let file_problems = session.store().file_problems().await?;
        for line in files_not_sent(report, &file_problems) {
            println!("{line}");
        }
        if !problems.is_empty() || !file_problems.is_empty() {
            println!("Run `dirt sync --verbose` for what releases each.");
        }
    }

    outcome
        .map(|_| ())
        .map_err(|error| CliError::Account(describe(&error)))
}
