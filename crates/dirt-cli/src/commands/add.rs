use crate::commands::common::{open_database, resolve_note_content, DbScope};
use crate::error::CliError;

pub async fn run_add(content_parts: &[String], scope: &DbScope) -> Result<(), CliError> {
    let content = resolve_note_content(content_parts)?;

    let db = open_database(scope).await?;
    let note = db.create_note(&content).await?;

    println!("{}", note.id);
    Ok(())
}
