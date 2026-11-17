use std::env;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use dirt_core::{Note, NoteId, Store};
use serde::Serialize;

use crate::error::CliError;

#[derive(Debug, Serialize)]
pub struct NoteListItem {
    pub id: String,
    pub preview: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub relative_time: String,
    pub tags: Vec<String>,
    pub archived: bool,
}

/// The list, or with `archived` the archived notes and not the others.
/// Either can be narrowed to a tag.
pub async fn list_notes(
    limit: usize,
    tag: Option<&str>,
    archived: bool,
    db: &Store,
) -> Result<Vec<Note>, CliError> {
    Ok(match (tag, archived) {
        (Some(tag), false) => db.list_notes_by_tag(tag, limit, 0).await?,
        (Some(tag), true) => db.list_archived_notes_by_tag(tag, limit, 0).await?,
        (None, false) => db.list_notes(limit, 0).await?,
        (None, true) => db.list_archived_notes(limit, 0).await?,
    })
}

pub async fn search_notes(query: &str, limit: usize, db: &Store) -> Result<Vec<Note>, CliError> {
    Ok(db.search_notes(query, limit).await?)
}

pub async fn resolve_note_for_edit(note_query: &str, db: &Store) -> Result<Note, CliError> {
    if let Ok(note_id) = note_query.parse::<NoteId>() {
        if let Some(note) = db.get_note(&note_id).await? {
            return Ok(note);
        }
    }

    let matching_ids = db.list_note_ids_by_prefix(note_query, 3).await?;
    let resolved_id = the_one_of(&matching_ids, note_query)?;
    db.get_note(&resolved_id)
        .await?
        .ok_or_else(|| CliError::NoteNotFound(note_query.to_string()))
}

/// The ID of the deleted note that `note_query` names, for bringing it
/// back: a whole ID, or the start of the ID of one deleted note that this
/// device still holds.
///
/// A whole ID is taken as it is. The store says if it holds no such note,
/// and leaves alone a note that is not deleted.
pub async fn resolve_deleted_note(note_query: &str, db: &Store) -> Result<NoteId, CliError> {
    if let Ok(note_id) = note_query.parse::<NoteId>() {
        return Ok(note_id);
    }
    let matching_ids = db.list_deleted_note_ids_by_prefix(note_query, 3).await?;
    the_one_of(&matching_ids, note_query)
}

/// The one ID that a prefix matched, or why there is not exactly one.
fn the_one_of(matching_ids: &[String], note_query: &str) -> Result<NoteId, CliError> {
    match matching_ids {
        [] => Err(CliError::NoteNotFound(note_query.to_string())),
        [only] => only
            .parse::<NoteId>()
            .map_err(|_| CliError::NoteNotFound(note_query.to_string())),
        several => {
            let options = several
                .iter()
                .take(3)
                .map(|id| id.chars().take(13).collect::<String>())
                .collect::<Vec<_>>()
                .join(", ");

            Err(CliError::AmbiguousNoteId(format!(
                "ID prefix '{note_query}' is ambiguous; matches: {options}"
            )))
        }
    }
}

pub fn format_note_lines(notes: &[Note]) -> Vec<String> {
    let now_ms = Utc::now().timestamp_millis();
    notes
        .iter()
        .map(|note| {
            let id = note.id.to_string();
            let short_id = id.chars().take(13).collect::<String>();
            let preview = note_preview(note, 40);
            let relative_time = format_relative_time(note.updated_at, now_ms);
            // After the time: the note's tags, and a word for an archived
            // note, which a search lists among the others.
            let mut rest = render_tags(note);
            if note.archived {
                if !rest.is_empty() {
                    rest.push_str("  ");
                }
                rest.push_str(ARCHIVED_MARK);
            }

            if rest.is_empty() {
                format!("{short_id:<13}  {preview:<40}  {relative_time}")
            } else {
                format!("{short_id:<13}  {preview:<40}  {relative_time:<10}  {rest}")
            }
        })
        .collect()
}

/// What a line of a list says of an archived note.
pub const ARCHIVED_MARK: &str = "(archived)";

pub fn note_to_list_item(note: &Note) -> NoteListItem {
    let now_ms = Utc::now().timestamp_millis();
    let mut tags = note.tags();
    tags.sort();

    NoteListItem {
        id: note.id.to_string(),
        preview: note_preview(note, 80),
        content: note.content.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
        relative_time: format_relative_time(note.updated_at, now_ms),
        tags,
        archived: note.archived,
    }
}

pub fn note_preview(note: &Note, max_chars: usize) -> String {
    let first_line = note.content.lines().next().unwrap_or("").trim();
    let collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");

    if collapsed.chars().count() <= max_chars {
        collapsed
    } else {
        let take_len = max_chars.saturating_sub(3);
        let mut truncated = collapsed.chars().take(take_len).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

pub fn render_tags(note: &Note) -> String {
    let mut tags = note.tags();
    tags.sort();
    tags.into_iter()
        .map(|tag| format!("#{tag}"))
        .collect::<Vec<String>>()
        .join(" ")
}

pub fn format_relative_time(timestamp_ms: i64, now_ms: i64) -> String {
    let diff = now_ms.saturating_sub(timestamp_ms);
    let minute = 60_000;
    let hour = 60 * minute;
    let day = 24 * hour;
    let week = 7 * day;
    let month = 30 * day;
    let year = 365 * day;

    if diff < minute {
        "just now".to_string()
    } else if diff < hour {
        format!("{}m ago", diff / minute)
    } else if diff < day {
        format!("{}h ago", diff / hour)
    } else if diff < week {
        format!("{}d ago", diff / day)
    } else if diff < month {
        format!("{}w ago", diff / week)
    } else if diff < year {
        format!("{}mo ago", diff / month)
    } else {
        format!("{}y ago", diff / year)
    }
}

pub fn resolve_note_content(content_parts: &[String]) -> Result<String, CliError> {
    if let Some(content) = normalize_content(&content_parts.join(" ")) {
        return Ok(content);
    }

    if let Some(content) = read_piped_stdin()? {
        return Ok(content);
    }

    if let Some(content) = capture_editor_input()? {
        return Ok(content);
    }

    Err(CliError::EmptyContent)
}

pub fn normalize_content(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn normalize_search_query(query: &str) -> Result<String, CliError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        Err(CliError::EmptySearchQuery)
    } else {
        Ok(trimmed.to_string())
    }
}

pub fn normalize_note_identifier(id: &str) -> Result<String, CliError> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        Err(CliError::EmptyNoteId)
    } else {
        Ok(trimmed.to_string())
    }
}

pub fn read_piped_stdin() -> Result<Option<String>, CliError> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }

    let mut buffer = String::new();
    stdin.lock().read_to_string(&mut buffer)?;
    Ok(normalize_content(&buffer))
}

pub fn capture_editor_input() -> Result<Option<String>, CliError> {
    capture_editor_input_with_initial("")
}

pub fn capture_editor_input_with_initial(
    initial_content: &str,
) -> Result<Option<String>, CliError> {
    let editor = preferred_editor();
    let temp_file = create_temp_note_file_path();
    std::fs::write(&temp_file, initial_content)?;

    let launch_result = launch_editor(&editor, &temp_file);
    let note_content = std::fs::read_to_string(&temp_file)?;
    let _ = std::fs::remove_file(&temp_file);

    launch_result?;
    Ok(normalize_content(&note_content))
}

/// Runs the editor on `file_path` and waits for it.
///
/// `editor` is first taken as the name of a program, which lets a path
/// with spaces in it work. If no program can be started under that name,
/// it is taken as a program followed by arguments, as in `code --wait`.
/// Whatever the first attempt's failure was, the second is made: Windows
/// reports a name it cannot use, such as one holding a drive's colon after
/// an argument, as something other than "not found".
pub fn launch_editor(editor: &str, file_path: &Path) -> Result<(), CliError> {
    match Command::new(editor).arg(file_path).status() {
        Ok(status) => {
            if status.success() {
                Ok(())
            } else {
                Err(CliError::EditorFailed(format!(
                    "`{editor}` exited with status {status}"
                )))
            }
        }
        Err(_) if editor.split_whitespace().count() != 1 => {
            let mut parts = editor.split_whitespace();
            let Some(program) = parts.next() else {
                return Err(CliError::EditorFailed("empty EDITOR command".into()));
            };

            let mut command = Command::new(program);
            command.args(parts).arg(file_path);

            let status = command.status()?;
            if status.success() {
                Ok(())
            } else {
                Err(CliError::EditorFailed(format!(
                    "`{editor}` exited with status {status}"
                )))
            }
        }
        Err(err) => Err(CliError::Io(err)),
    }
}

pub fn preferred_editor() -> String {
    env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| default_editor().to_string())
}

pub const fn default_editor() -> &'static str {
    if cfg!(windows) {
        "notepad"
    } else {
        "vi"
    }
}

pub fn create_temp_note_file_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    env::temp_dir().join(format!("dirt-note-{}-{now}.md", std::process::id()))
}
