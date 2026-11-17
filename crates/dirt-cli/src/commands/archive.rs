//! `dirt archive` and `dirt unarchive`: a note out of the list, and back.
//!
//! Archiving is not an edit. The note keeps its text and its edit time, is
//! still found by `dirt search`, and is listed by `dirt list --archived`.

use crate::commands::common::{normalize_note_identifier, resolve_note_for_edit};
use crate::context::Context;
use crate::error::CliError;

pub async fn run_archive(id: &str, context: &Context) -> Result<(), CliError> {
    set_archived(id, true, context).await
}

pub async fn run_unarchive(id: &str, context: &Context) -> Result<(), CliError> {
    set_archived(id, false, context).await
}

/// Prints the note's ID, as `dirt delete` does. A note that is already as
/// asked is left as it is, and nothing is synced for it.
async fn set_archived(id: &str, archived: bool, context: &Context) -> Result<(), CliError> {
    let normalized_id = normalize_note_identifier(id)?;
    let db = &context.store;
    let note = resolve_note_for_edit(&normalized_id, db).await?;

    println!("{}", note.id);
    if note.archived == archived {
        return Ok(());
    }
    if archived {
        db.archive_note(&note.id).await?;
    } else {
        db.unarchive_note(&note.id).await?;
    }
    context.sync_after_write().await;
    Ok(())
}
