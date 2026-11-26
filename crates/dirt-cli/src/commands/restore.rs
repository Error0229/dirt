//! `dirt restore`: a deleted note back, with the text it had.
//!
//! A deleted note is kept on the device, marked deleted, until it is
//! purged thirty days later. Until then its ID still names it, and
//! `dirt delete` printed that ID; so does its number in the list it was
//! deleted from, while that is the last list.

use crate::commands::common::report;
use crate::context::Context;
use crate::error::CliError;
use crate::naming::deleted_note_named;

pub async fn run_restore(note: &[String], context: &Context) -> Result<(), CliError> {
    let db = &context.store;
    let note_id = deleted_note_named(note, db).await?;

    let note = match db.restore_note(&note_id).await {
        Ok(note) => note,
        // A whole ID of a note this device does not hold, or holds no
        // longer.
        Err(dirt_core::Error::NotFound(_)) => {
            return Err(CliError::NoteNotFound(format!(
                "Note {} is not on this device",
                note.join(" ").trim()
            )))
        }
        Err(other) => return Err(other.into()),
    };
    report("Restored", &note, None);
    context.sync_after_write().await;
    Ok(())
}
