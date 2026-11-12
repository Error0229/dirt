//! Exporting every note to one file.
//!
//! The file's contents are `dirt-core`'s, the same as the desktop app and
//! the CLI write. Where the file goes is the person's choice, made on
//! Android's own screen for that (`services::activity::save_file`), so this
//! module only makes the file's name and text.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use dirt_core::export::{render_notes_export, suggested_export_file_name, ExportFormat};
use thiserror::Error;

use crate::data::MobileNoteStore;

#[derive(Debug, Error)]
pub enum MobileExportError {
    #[error(transparent)]
    Database(#[from] dirt_core::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
}

/// A file ready to be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFile {
    /// The name offered to the person, which they may change.
    pub name: String,
    pub mime_type: &'static str,
    pub text: String,
    /// How many notes the file holds.
    pub notes: usize,
}

/// Every note in `store`, as a file in `format`.
pub async fn export_file(
    store: &MobileNoteStore,
    format: ExportFormat,
    now_ms: i64,
) -> Result<ExportFile, MobileExportError> {
    let notes = store.list_all_notes().await?;
    Ok(ExportFile {
        name: suggested_export_file_name(format, now_ms),
        mime_type: match format {
            ExportFormat::Json => "application/json",
            ExportFormat::Markdown => "text/markdown",
        },
        text: render_notes_export(&notes, format)?,
        notes: notes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_holds_every_note_under_a_name_for_its_format() {
        let (_folder, store) = MobileNoteStore::open_temporary();
        store.create_note("Mobile export #one").await.unwrap();
        store.create_note("Mobile export #two").await.unwrap();

        let markdown = export_file(&store, ExportFormat::Markdown, 456)
            .await
            .unwrap();
        assert_eq!(markdown.name, "dirt-export-456.md");
        assert_eq!(markdown.mime_type, "text/markdown");
        assert_eq!(markdown.notes, 2);
        assert!(markdown.text.contains("Mobile export #one"));
        assert!(markdown.text.contains("Mobile export #two"));

        let json = export_file(&store, ExportFormat::Json, 123).await.unwrap();
        assert_eq!(json.name, "dirt-export-123.json");
        assert_eq!(json.mime_type, "application/json");
        let parsed: serde_json::Value = serde_json::from_str(&json.text).unwrap();
        assert_eq!(parsed.as_array().map(Vec::len), Some(2));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_export_reaches_past_the_notes_the_list_shows() {
        let (_folder, store) = MobileNoteStore::open_temporary();
        for number in 0..130 {
            store.create_note(&format!("note {number}")).await.unwrap();
        }
        let exported = export_file(&store, ExportFormat::Json, 1).await.unwrap();
        assert_eq!(exported.notes, 130);
    }
}
