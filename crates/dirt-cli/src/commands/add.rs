use crate::commands::common::{report, resolve_note_content};
use crate::context::Context;
use crate::error::CliError;

pub async fn run_add(content_parts: &[String], context: &Context) -> Result<(), CliError> {
    let content = resolve_note_content(content_parts)?;

    let note = context.store.create_note(&content).await?;

    // The note is saved and said before anything touches the network:
    // capture never waits for sync.
    report("Saved", &note, None);
    context.sync_after_write().await;
    Ok(())
}
