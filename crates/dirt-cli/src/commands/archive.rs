//! `dirt archive` and `dirt unarchive`: a note out of the list, and back.
//!
//! Archiving is not an edit. The note keeps its text and its edit time, is
//! still found by `dirt search`, and is listed by `dirt list --archived`.

use crate::commands::common::report;
use crate::context::Context;
use crate::error::CliError;
use crate::naming::note_named;

pub async fn run_archive(note: &[String], context: &Context) -> Result<(), CliError> {
    set_archived(note, true, context).await
}

pub async fn run_unarchive(note: &[String], context: &Context) -> Result<(), CliError> {
    set_archived(note, false, context).await
}

/// Says what was done, as `dirt delete` does. A note that is already as
/// asked is left as it is, and nothing is synced for it.
async fn set_archived(note: &[String], archived: bool, context: &Context) -> Result<(), CliError> {
    let db = &context.store;
    let note = note_named(note, context).await?;

    if note.archived == archived {
        let already = if archived {
            "Already archived"
        } else {
            "Not archived"
        };
        report(already, &note, None);
        return Ok(());
    }
    let note = if archived {
        db.archive_note(&note.id).await?
    } else {
        db.unarchive_note(&note.id).await?
    };
    let done = if archived {
        "Archived"
    } else {
        "Back in the list"
    };
    report(done, &note, None);
    context.sync_after_write().await;
    Ok(())
}
