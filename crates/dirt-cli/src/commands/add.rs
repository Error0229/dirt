use std::path::Path;

use dirt_core::db::{LibSqlNoteRepository, NoteRepository};

use crate::commands::common::{open_database, resolve_note_content};
use crate::error::CliError;

pub async fn run_add(content_parts: &[String], db_path: &Path) -> Result<(), CliError> {
    let content = resolve_note_content(content_parts)?;

    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());
    let note = repo.create(&content).await?;

    println!("{}", note.id);
    Ok(())
}
