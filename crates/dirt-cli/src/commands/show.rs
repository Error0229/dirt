//! `dirt show`: a whole note.
//!
//! Its text first, as it is, then a blank line and what there is to know
//! about it, one thing to a line: its ID, when it was edited and written,
//! its tags, its files, and whether it is archived.

use chrono::{Local, TimeZone, Utc};
use dirt_core::store::AttachedFile;
use dirt_core::sync::overview::size_words;
use dirt_core::Note;
use serde::Serialize;

use crate::commands::common::{
    format_relative_time, note_preview, note_to_list_item, render_tags, NoteListItem,
};
use crate::commands::files::{file_item, FileListItem};
use crate::context::Context;
use crate::error::CliError;
use crate::naming::note_named_or_latest;

/// A note as `dirt show --json` gives it: as `dirt list --json` gives it,
/// with its files as `dirt files --json` gives them.
#[derive(Debug, Serialize)]
struct ShownNote {
    #[serde(flatten)]
    note: NoteListItem,
    files: Vec<FileListItem>,
}

pub async fn run_show(note: &[String], as_json: bool, context: &Context) -> Result<(), CliError> {
    let note = note_named_or_latest(note, context).await?;
    let files = context.store.attached_files(&note.id).await?;
    if as_json {
        let shown = ShownNote {
            note: note_to_list_item(&note),
            files: files.into_iter().map(file_item).collect(),
        };
        println!("{}", serde_json::to_string_pretty(&shown)?);
    } else {
        for line in show_lines(&note, &files, Utc::now().timestamp_millis()) {
            println!("{line}");
        }
    }
    Ok(())
}

/// What `dirt show` prints of a note, a line at a time.
pub fn show_lines(note: &Note, files: &[AttachedFile], now_ms: i64) -> Vec<String> {
    let mut lines: Vec<String> = if note.content.trim().is_empty() {
        // A thought said aloud whose words are not made yet: its name, as
        // the list gives it.
        vec![note_preview(note, usize::MAX)]
    } else {
        note.content.lines().map(str::to_owned).collect()
    };
    lines.push(String::new());
    let mut about = |label: &str, value: &str| lines.push(format!("{label:<10}{value}"));
    about("ID", &note.id.to_string());
    about(
        "Edited",
        &format!(
            "{}, {}",
            format_relative_time(note.updated_at, now_ms),
            local_time(note.updated_at)
        ),
    );
    about("Written", &local_time(note.created_at));
    let tags = render_tags(note);
    if !tags.is_empty() {
        about("Tags", &tags);
    }
    for (index, file) in files.iter().enumerate() {
        let attachment = &file.attachment;
        let mut value = format!(
            "{}  {}  {}",
            attachment.name,
            size_words(attachment.size),
            attachment.media_type
        );
        if let Some(words) = file.standing_words() {
            value.push_str("  (");
            value.push_str(&words);
            value.push(')');
        }
        about(if index == 0 { "Files" } else { "" }, &value);
    }
    if note.archived {
        about("Archived", "yes; `dirt unarchive` puts it back in the list");
    }
    lines
}

/// A time on this computer's clock, to the minute.
fn local_time(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or_else(String::new, |time| {
            time.format("%-d %b %Y %H:%M").to_string()
        })
}
