//! `dirt restore`: a deleted note back, with the text it had.
//!
//! A deleted note is kept on the device, marked deleted, until it is
//! purged thirty days later. Until then its ID still names it, and
//! `dirt delete` printed that ID.

use crate::commands::common::{normalize_note_identifier, resolve_deleted_note};
use crate::context::Context;
use crate::error::CliError;

pub async fn run_restore(id: &str, context: &Context) -> Result<(), CliError> {
    let normalized_id = normalize_note_identifier(id)?;
    let db = &context.store;
    let note_id = resolve_deleted_note(&normalized_id, db).await?;

    let note = match db.restore_note(&note_id).await {
        Ok(note) => note,
        // A whole ID of a note this device does not hold, or holds no
        // longer.
        Err(dirt_core::Error::NotFound(_)) => return Err(CliError::NoteNotFound(normalized_id)),
        Err(other) => return Err(other.into()),
    };
    println!("{}", note.id);
    context.sync_after_write().await;
    Ok(())
}
