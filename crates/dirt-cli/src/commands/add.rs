use crate::commands::common::resolve_note_content;
use crate::context::Context;
use crate::error::CliError;

pub async fn run_add(content_parts: &[String], context: &Context) -> Result<(), CliError> {
    let content = resolve_note_content(content_parts)?;

    let note = context.store.create_note(&content).await?;

    // The note is saved and its ID printed before anything touches the
    // network: capture never waits for sync.
    println!("{}", note.id);
    context.sync_after_write().await;
    Ok(())
}
