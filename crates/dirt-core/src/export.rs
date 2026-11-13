//! Exporting every note, with the files attached to them or without.
//!
//! An export is the notes as one text, JSON or Markdown, and with it, when
//! it is written to a folder or as one zip file, the notes' files:
//!
//! ```text
//! notes.json                      or notes.md
//! files/<note id>/<file name>     one for each file of each note
//! ```
//!
//! The text names every file of every note. A file the export holds has a
//! `path`, from the folder the text is in. A file it does not hold has
//! none: the text alone was asked for, or the device did not have the file
//! and could not fetch it. So an export never lets a file go unsaid.
//!
//! This module is the text. [`Export`] gathers the notes and their files
//! from a store and writes them.

mod bundle;

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::Note;

pub use bundle::{Export, ExportError, LeftOut};

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
    /// The files attached to the note, in the order they were attached.
    /// Empty for a note with none, and for an export made before notes
    /// had files.
    #[serde(default)]
    pub files: Vec<ExportFile>,
}

/// One file attached to a note, as an export names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFile {
    pub id: String,
    /// The file's name as the note gives it. Its name in the export may
    /// differ: see `path`.
    pub name: String,
    pub media_type: String,
    /// The size of the file in bytes.
    pub size: u64,
    /// Where the file is in the export, from the folder the notes' text is
    /// in, with `/` between the parts. `None` when the export does not
    /// hold the file.
    pub path: Option<String>,
}

/// Convert a note into an export record with stable tag ordering. The
/// record names no files: [`Export`] adds them.
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
        files: Vec::new(),
    }
}

/// Render notes as pretty-printed JSON.
pub fn render_json_export(notes: &[ExportNote]) -> serde_json::Result<String> {
    serde_json::to_string_pretty(notes)
}

/// Render notes in Markdown with frontmatter blocks.
///
/// A note with files has them listed in its frontmatter. Each value there
/// that a person typed, a file's name above all, is written as a JSON
/// string, which YAML reads as the same text: a name may hold a colon, a
/// quote or a `#`, and bare, any of them would change what the block says.
#[must_use]
pub fn render_markdown_export(notes: &[ExportNote]) -> String {
    let mut output = String::new();

    for (index, note) in notes.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }

        let _ = writeln!(output, "---");
        let _ = writeln!(output, "id: {}", note.id);
        let _ = writeln!(output, "created_at: {}", note.created_at);
        let _ = writeln!(output, "updated_at: {}", note.updated_at);
        let _ = writeln!(output, "tags:");
        for tag in &note.tags {
            let _ = writeln!(output, "  - {tag}");
        }
        if !note.files.is_empty() {
            let _ = writeln!(output, "files:");
        }
        for file in &note.files {
            let _ = writeln!(output, "  - name: {}", quoted(&file.name));
            match &file.path {
                Some(path) => {
                    let _ = writeln!(output, "    path: {}", quoted(path));
                }
                None => {
                    let _ = writeln!(output, "    path: null");
                }
            }
            let _ = writeln!(output, "    media_type: {}", quoted(&file.media_type));
            let _ = writeln!(output, "    size: {}", file.size);
            let _ = writeln!(output, "    id: {}", file.id);
        }
        let _ = writeln!(output, "---");
        let _ = writeln!(output);
        output.push_str(&note.content);
        output.push('\n');
    }

    output
}

/// `text` as a JSON string, quotes and all.
fn quoted(text: &str) -> String {
    // A string always serializes. The fallback is never reached, and is a
    // value that still reads as a string.
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// Render notes based on selected export format.
pub fn render_notes_export(
    notes: &[ExportNote],
    format: ExportFormat,
) -> serde_json::Result<String> {
    match format {
        ExportFormat::Json => render_json_export(notes),
        ExportFormat::Markdown => Ok(render_markdown_export(notes)),
    }
}

/// A name for an export that is one text: `dirt-export-<time>.json`.
#[must_use]
pub fn suggested_export_file_name(format: ExportFormat, timestamp_ms: i64) -> String {
    format!("dirt-export-{timestamp_ms}.{}", format.extension())
}

/// A name for the folder an export with files is written to:
/// `dirt-export-<time>`.
#[must_use]
pub fn suggested_export_folder_name(timestamp_ms: i64) -> String {
    format!("dirt-export-{timestamp_ms}")
}

/// A name for an export with files written as one file:
/// `dirt-export-<time>.zip`.
#[must_use]
pub fn suggested_export_zip_name(timestamp_ms: i64) -> String {
    format!("dirt-export-{timestamp_ms}.zip")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_with_files(files: Vec<ExportFile>) -> ExportNote {
        ExportNote {
            id: "cccccccc-cccc-7ccc-8ccc-111111111111".to_owned(),
            content: "Hello export #tag".to_owned(),
            created_at: 123,
            updated_at: 456,
            tags: vec!["tag".to_owned()],
            files,
        }
    }

    #[test]
    fn note_to_export_item_sorts_tags() {
        let note = Note::new("#zeta test #alpha #beta");
        let export = note_to_export_item(&note);

        assert_eq!(export.tags, vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn render_markdown_export_includes_frontmatter_and_content() {
        let rendered = render_markdown_export(&[note_with_files(Vec::new())]);
        assert_eq!(
            rendered,
            "---\n\
             id: cccccccc-cccc-7ccc-8ccc-111111111111\n\
             created_at: 123\n\
             updated_at: 456\n\
             tags:\n  - tag\n\
             ---\n\n\
             Hello export #tag\n"
        );
    }

    #[test]
    fn markdown_lists_a_notes_files_and_says_which_the_export_holds() {
        let rendered = render_markdown_export(&[note_with_files(vec![
            ExportFile {
                id: "aaaaaaaa-aaaa-7aaa-8aaa-000000000001".to_owned(),
                name: "plan.pdf".to_owned(),
                media_type: "application/pdf".to_owned(),
                size: 2048,
                path: Some("files/cccccccc-cccc-7ccc-8ccc-111111111111/plan.pdf".to_owned()),
            },
            ExportFile {
                id: "aaaaaaaa-aaaa-7aaa-8aaa-000000000002".to_owned(),
                name: "away.png".to_owned(),
                media_type: "image/png".to_owned(),
                size: 7,
                path: None,
            },
        ])]);
        assert!(rendered.contains(
            "tags:\n  - tag\n\
             files:\n  \
             - name: \"plan.pdf\"\n    \
             path: \"files/cccccccc-cccc-7ccc-8ccc-111111111111/plan.pdf\"\n    \
             media_type: \"application/pdf\"\n    \
             size: 2048\n    \
             id: aaaaaaaa-aaaa-7aaa-8aaa-000000000001\n  \
             - name: \"away.png\"\n    \
             path: null\n    \
             media_type: \"image/png\"\n    \
             size: 7\n    \
             id: aaaaaaaa-aaaa-7aaa-8aaa-000000000002\n\
             ---\n"
        ));
    }

    #[test]
    fn a_file_name_cannot_change_what_the_frontmatter_says() {
        let rendered = render_markdown_export(&[note_with_files(vec![ExportFile {
            id: "aaaaaaaa-aaaa-7aaa-8aaa-000000000001".to_owned(),
            name: "a: b\n---\nid: forged # \"x\"".to_owned(),
            media_type: "text/plain".to_owned(),
            size: 1,
            path: None,
        }])]);
        // The name is on one line, inside quotes, with its own quote and
        // line breaks escaped.
        assert!(rendered.contains("  - name: \"a: b\\n---\\nid: forged # \\\"x\\\"\"\n"));
        // So the block still has one start and one end.
        assert_eq!(rendered.matches("\n---\n").count(), 1);
    }

    #[test]
    fn json_names_each_file_and_reads_an_export_made_before_files() {
        let note = note_with_files(vec![ExportFile {
            id: "aaaaaaaa-aaaa-7aaa-8aaa-000000000001".to_owned(),
            name: "plan.pdf".to_owned(),
            media_type: "application/pdf".to_owned(),
            size: 2048,
            path: None,
        }]);
        let rendered = render_json_export(std::slice::from_ref(&note)).unwrap();
        let read: Vec<ExportNote> = serde_json::from_str(&rendered).unwrap();
        assert_eq!(read, vec![note]);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert!(parsed[0]["files"][0]["path"].is_null());

        let before: Vec<ExportNote> = serde_json::from_str(
            r#"[{"id":"x","content":"old","created_at":1,"updated_at":2,"tags":[]}]"#,
        )
        .unwrap();
        assert!(before[0].files.is_empty());
    }

    #[test]
    fn suggested_names_say_what_the_export_is() {
        assert_eq!(
            suggested_export_file_name(ExportFormat::Json, 123),
            "dirt-export-123.json"
        );
        assert_eq!(
            suggested_export_file_name(ExportFormat::Markdown, 456),
            "dirt-export-456.md"
        );
        assert_eq!(suggested_export_folder_name(789), "dirt-export-789");
        assert_eq!(suggested_export_zip_name(789), "dirt-export-789.zip");
    }
}
