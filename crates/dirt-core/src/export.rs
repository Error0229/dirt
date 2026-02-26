//! Shared note export helpers for CLI/Desktop/Mobile parity.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::Note;

/// Export output format shared by all clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Json,
    Markdown,
}

impl ExportFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "md",
        }
    }
}

/// Serializable note representation used in JSON and Markdown exports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportNote {
    pub id: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub tags: Vec<String>,
}

/// Convert a note into an export record with stable tag ordering.
#[must_use]
pub fn note_to_export_item(note: &Note) -> ExportNote {
    let mut tags = note.tags();
    tags.sort();

    ExportNote {
        id: note.id.to_string(),
        content: note.content.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
        tags,
    }
}

/// Render notes as pretty-printed JSON.
pub fn render_json_export(notes: &[Note]) -> serde_json::Result<String> {
    let items = notes
        .iter()
        .map(note_to_export_item)
        .collect::<Vec<ExportNote>>();
    serde_json::to_string_pretty(&items)
}

/// Render notes in Markdown with frontmatter blocks.
#[must_use]
pub fn render_markdown_export(notes: &[Note]) -> String {
    let mut output = String::new();

    for (index, note) in notes.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }

        let export_note = note_to_export_item(note);
        let _ = writeln!(output, "---");
        let _ = writeln!(output, "id: {}", export_note.id);
        let _ = writeln!(output, "created_at: {}", export_note.created_at);
        let _ = writeln!(output, "updated_at: {}", export_note.updated_at);
        let _ = writeln!(output, "tags:");
        for tag in export_note.tags {
            let _ = writeln!(output, "  - {tag}");
        }
        let _ = writeln!(output, "---");
        let _ = writeln!(output);
        output.push_str(&export_note.content);
        output.push('\n');
    }

    output
}

/// Render notes based on selected export format.
pub fn render_notes_export(notes: &[Note], format: ExportFormat) -> serde_json::Result<String> {
    match format {
        ExportFormat::Json => render_json_export(notes),
        ExportFormat::Markdown => Ok(render_markdown_export(notes)),
    }
}

/// Build a deterministic default file name for export flows.
#[must_use]
pub fn suggested_export_file_name(format: ExportFormat, timestamp_ms: i64) -> String {
    format!("dirt-export-{timestamp_ms}.{}", format.extension())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_to_export_item_sorts_tags() {
        let note = Note::new("#zeta test #alpha #beta");
        let export = note_to_export_item(&note);

        assert_eq!(export.tags, vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn render_markdown_export_includes_frontmatter_and_content() {
        let note = Note {
            id: "cccccccc-cccc-7ccc-8ccc-111111111111".parse().unwrap(),
            content: "Hello export #tag".to_string(),
            created_at: 123,
            updated_at: 456,
            is_deleted: false,
        };

        let rendered = render_markdown_export(&[note]);
        assert!(rendered.contains("id: cccccccc-cccc-7ccc-8ccc-111111111111"));
        assert!(rendered.contains("created_at: 123"));
        assert!(rendered.contains("updated_at: 456"));
        assert!(rendered.contains("tags:\n  - tag"));
        assert!(rendered.contains("Hello export #tag"));
    }

    #[test]
    fn suggested_export_file_name_uses_format_extension() {
        assert_eq!(
            suggested_export_file_name(ExportFormat::Json, 123),
            "dirt-export-123.json"
        );
        assert_eq!(
            suggested_export_file_name(ExportFormat::Markdown, 456),
            "dirt-export-456.md"
        );
    }
}
