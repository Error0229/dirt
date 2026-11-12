use crate::commands::common::{normalize_note_identifier, resolve_note_for_edit};
use crate::context::Context;
use crate::error::CliError;

pub async fn run_delete(id: &str, context: &Context) -> Result<(), CliError> {
    let normalized_id = normalize_note_identifier(id)?;
    let db = &context.store;
    let note = resolve_note_for_edit(&normalized_id, db).await?;

    db.delete_note(&note.id).await?;
    println!("{}", note.id);
    context.sync_after_write().await;
    Ok(())
}
