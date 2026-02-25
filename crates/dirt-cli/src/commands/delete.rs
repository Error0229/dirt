use std::path::Path;

use dirt_core::db::{LibSqlNoteRepository, NoteRepository};

use crate::commands::common::{normalize_note_identifier, open_database, resolve_note_for_edit};
use crate::error::CliError;

pub async fn run_delete(id: &str, db_path: &Path) -> Result<(), CliError> {
    let normalized_id = normalize_note_identifier(id)?;
    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());
    let note = resolve_note_for_edit(&normalized_id, &db).await?;

    repo.delete(&note.id).await?;
    println!("{}", note.id);
    Ok(())
}
