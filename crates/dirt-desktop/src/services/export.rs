//! Note export service for desktop UI parity with CLI exports.

use std::path::Path;

use dirt_core::export::{
    render_notes_export, suggested_export_file_name as core_suggested_export_file_name,
    ExportFormat,
};
use dirt_core::Note;
use thiserror::Error;

use super::DatabaseService;

const PAGE_SIZE: usize = 500;

/// Export output format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotesExportFormat {
    Json,
    Markdown,
}

impl From<NotesExportFormat> for ExportFormat {
    fn from(value: NotesExportFormat) -> Self {
        match value {
            NotesExportFormat::Json => Self::Json,
            NotesExportFormat::Markdown => Self::Markdown,
        }
    }
}

/// Errors emitted by desktop note export flows.
#[derive(Debug, Error)]
pub enum NotesExportError {
    #[error(transparent)]
    Database(#[from] dirt_core::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Export all non-deleted notes to the destination path.
pub async fn export_notes_to_path(
    db: &DatabaseService,
    format: NotesExportFormat,
    output_path: &Path,
) -> Result<usize, NotesExportError> {
    let notes = list_all_notes(db).await?;
    let rendered = render_notes_export(&notes, format.into())?;

    std::fs::write(output_path, rendered)?;
    Ok(notes.len())
}

/// Build a deterministic default file name for save dialogs.
#[must_use]
pub fn suggested_export_file_name(format: NotesExportFormat, timestamp_ms: i64) -> String {
    core_suggested_export_file_name(format.into(), timestamp_ms)
}

async fn list_all_notes(db: &DatabaseService) -> Result<Vec<Note>, dirt_core::Error> {
    let mut notes = Vec::new();
    let mut offset = 0usize;

    loop {
        let batch = db.list_notes(PAGE_SIZE, offset).await?;
        let count = batch.len();
        notes.extend(batch);

        if count < PAGE_SIZE {
            break;
        }
        offset += count;
    }

    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::DatabaseService;

    #[test]
    fn suggested_export_file_name_uses_format_extension() {
        assert_eq!(
            suggested_export_file_name(NotesExportFormat::Json, 123),
            "dirt-export-123.json"
        );
        assert_eq!(
            suggested_export_file_name(NotesExportFormat::Markdown, 456),
            "dirt-export-456.md"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn export_notes_to_path_writes_markdown() {
        let db = DatabaseService::in_memory().await.unwrap();
        db.create_note("Desktop export #one").await.unwrap();
        db.create_note("Desktop export #two").await.unwrap();

        let output_path = std::env::temp_dir().join(format!(
            "dirt-desktop-export-test-{}.md",
            chrono::Utc::now().timestamp_millis()
        ));

        let exported_count = export_notes_to_path(&db, NotesExportFormat::Markdown, &output_path)
            .await
            .unwrap();
        assert_eq!(exported_count, 2);

        let exported = std::fs::read_to_string(&output_path).unwrap();
        assert!(exported.contains("Desktop export #one"));
        assert!(exported.contains("Desktop export #two"));

        let _ = std::fs::remove_file(output_path);
    }
}
