//! Mobile note export helpers.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dirt_core::export::{render_json_export, render_markdown_export};
use thiserror::Error;

use crate::data::MobileNoteStore;
use crate::paths::dirt_data_dir;

const EXPORT_DIR_NAME: &str = "dirt-exports";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MobileExportFormat {
    Json,
    Markdown,
}

impl MobileExportFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "md",
        }
    }
}

#[derive(Debug, Error)]
pub enum MobileExportError {
    #[error(transparent)]
    Database(#[from] dirt_core::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub async fn export_notes_to_path(
    note_store: Arc<MobileNoteStore>,
    format: MobileExportFormat,
    output_path: &Path,
) -> Result<usize, MobileExportError> {
    let notes = note_store.list_all_notes().await?;
    let rendered = match format {
        MobileExportFormat::Json => render_json_export(&notes)?,
        MobileExportFormat::Markdown => render_markdown_export(&notes),
    };

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, rendered)?;
    Ok(notes.len())
}

#[must_use]
pub fn suggested_export_file_name(format: MobileExportFormat, timestamp_ms: i64) -> String {
    format!("dirt-export-{timestamp_ms}.{}", format.extension())
}

#[must_use]
pub fn default_export_directory() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::document_dir)
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(dirt_data_dir)
        .join(EXPORT_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_export_file_name_matches_format() {
        assert_eq!(
            suggested_export_file_name(MobileExportFormat::Json, 123),
            "dirt-export-123.json"
        );
        assert_eq!(
            suggested_export_file_name(MobileExportFormat::Markdown, 456),
            "dirt-export-456.md"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn export_notes_to_path_writes_markdown() {
        let store = Arc::new(MobileNoteStore::open_in_memory().await.unwrap());
        store.create_note("Mobile export #one").await.unwrap();
        store.create_note("Mobile export #two").await.unwrap();

        let output_path = std::env::temp_dir().join(format!(
            "dirt-mobile-export-test-{}.md",
            chrono::Utc::now().timestamp_millis()
        ));

        let exported_count =
            export_notes_to_path(store, MobileExportFormat::Markdown, &output_path)
                .await
                .unwrap();
        assert_eq!(exported_count, 2);

        let exported = std::fs::read_to_string(&output_path).unwrap();
        assert!(exported.contains("Mobile export #one"));
        assert!(exported.contains("Mobile export #two"));

        let _ = std::fs::remove_file(output_path);
    }
}
