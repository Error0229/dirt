//! `dirt sync`: one sync cycle, now.

use crate::context::{describe, Context};
use crate::error::CliError;

pub async fn run_sync(context: &Context) -> Result<(), CliError> {
    let session = context.require_session()?;
    let report = session
        .sync_now()
        .await
        .map_err(|error| CliError::Account(describe(&error)))?;

    println!(
        "Synced: {} received, {} sent.",
        report.pulled, report.pushed
    );
    if report.unsent > 0 {
        println!(
            "{} note(s) are still waiting; another device was changing them. Run `dirt sync` again.",
            report.unsent
        );
    }
    for problem in session.store().note_problems().await? {
        println!(
            "Note {} is not syncing: {}.",
            problem.note_id, problem.reason
        );
    }
    Ok(())
}
