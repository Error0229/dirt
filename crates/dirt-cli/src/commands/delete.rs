use crate::commands::common::report;
use crate::context::Context;
use crate::error::CliError;
use crate::naming::{note_named, SHORT_ID};

pub async fn run_delete(note: &[String], context: &Context) -> Result<(), CliError> {
    let db = &context.store;
    let note = note_named(note, context).await?;

    db.delete_note(&note.id).await?;
    // The start of its ID that names it among the deleted notes, which is
    // what `dirt restore` looks among.
    let short = db
        .shortest_id_prefixes(&[note.id], true, SHORT_ID)
        .await?
        .pop()
        .unwrap_or_else(|| note.id.to_string());
    report(
        "Deleted",
        &note,
        Some(&format!("`dirt restore {short}` brings it back")),
    );
    context.sync_after_write().await;
    Ok(())
}
