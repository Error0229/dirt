use std::env;
use std::io::{self, IsTerminal, Read};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use chrono::Utc;
use dirt_core::models::Words;
use dirt_core::{Note, NoteId, Store};
use serde::Serialize;

use crate::error::CliError;
use crate::naming::SHORT_ID;

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

/// The lines of a list: each note's number, the shortest start of its ID
/// that names it, its first words, when it was edited, its tags, and a
/// word for an archived note, which a search lists among the others.
pub async fn listing_lines(notes: &[Note], store: &Store) -> Result<Vec<String>, CliError> {
    let ids: Vec<NoteId> = notes.iter().map(|note| note.id).collect();
    let short_ids = store.shortest_id_prefixes(&ids, false, SHORT_ID).await?;
    Ok(format_note_lines(notes, &short_ids))
}

/// The lines of a list, given each note's short ID.
pub fn format_note_lines(notes: &[Note], short_ids: &[String]) -> Vec<String> {
    let now_ms = Utc::now().timestamp_millis();
    let number_width = notes.len().to_string().len();
    let id_width = short_ids.iter().map(String::len).max().unwrap_or(0);
    notes
        .iter()
        .zip(short_ids)
        .enumerate()
        .map(|(index, (note, short_id))| {
            let number = index + 1;
            let preview = note_preview(note, 40);
            let relative_time = format_relative_time(note.updated_at, now_ms);
            let mut rest = render_tags(note);
            if note.archived {
                if !rest.is_empty() {
                    rest.push_str("  ");
                }
                rest.push_str(ARCHIVED_MARK);
            }
            let line = format!(
                "{number:>number_width$}  {short_id:<id_width$}  {preview:<40}  {relative_time:<8}  {rest}"
            );
            line.trim_end().to_owned()
        })
        .collect()
}

/// What a command that changed a note says of it. To a terminal, in words:
/// what was done and the note's first words, and anything more there is
/// to say. Otherwise the note's whole ID alone, which is what a script
/// reads, as it always was.
pub fn report_line(done: &str, note: &Note, more: Option<&str>, to_terminal: bool) -> String {
    if !to_terminal {
        return note.id.to_string();
    }
    let preview = note_preview(note, 60);
    let mut line = if preview.is_empty() {
        format!("{done}.")
    } else {
        format!("{done}: {preview}")
    };
    if let Some(more) = more {
        line.push_str(if preview.is_empty() { " " } else { ". " });
        line.push_str(more);
    }
    line
}

/// Prints [`report_line`] on standard output.
pub fn report(done: &str, note: &Note, more: Option<&str>) {
    println!(
        "{}",
        report_line(done, note, more, io::stdout().is_terminal())
    );
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
    let mut collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    // A thought said aloud on a device is named by its length until that
    // device has made its words, as the apps name it (6.18).
    if collapsed.is_empty() {
        if let Some(voice) = note.voice {
            collapsed = match voice.words {
                Words::NoneFound => format!("Voice \u{b7} {} (no words found)", voice.length()),
                Words::Waiting | Words::Made => format!("Voice \u{b7} {}", voice.length()),
            };
        }
    }

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

    let edited = edit_in_editor(&preferred_editor(), "", &note_file_name(None))?;
    if let Some(content) = edited.text {
        return Ok(content);
    }
    if edited.at_once {
        eprintln!("{RETURNED_AT_ONCE}");
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

pub fn read_piped_stdin() -> Result<Option<String>, CliError> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }

    let mut buffer = String::new();
    stdin.lock().read_to_string(&mut buffer)?;
    Ok(normalize_content(&buffer))
}

/// What came back from the editor.
#[derive(Debug)]
pub struct Edited {
    /// The text, trimmed; `None` if nothing but space was left.
    pub text: Option<String>,
    /// The editor returned within [`AT_ONCE`]: perhaps it handed the file
    /// to a window already open and did not wait for it.
    pub at_once: bool,
}

/// An editor that returns sooner than this may not have waited for the
/// person.
pub const AT_ONCE: Duration = Duration::from_secs(1);

/// What to say when the editor came back at once with nothing changed.
pub const RETURNED_AT_ONCE: &str = "The editor returned at once. If it opens a window \
    and does not wait for it to close, set EDITOR to a command that waits, as in \
    `code --wait`";

/// The name of the file a note is edited in, which the editor shows: the
/// note's first words, or `new-note.txt` for a note not written yet. Plain
/// text, as a note is.
pub fn note_file_name(note: Option<&Note>) -> String {
    let Some(note) = note else {
        return "new-note.txt".to_owned();
    };
    let mut name = String::new();
    for c in note_preview(note, 40).trim_end_matches("...").chars() {
        if c.is_alphanumeric() {
            name.extend(c.to_lowercase());
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_end_matches('-');
    if name.is_empty() {
        "note.txt".to_owned()
    } else if is_reserved_on_windows(name) {
        // `con.txt` is the console on Windows, not a file.
        format!("note-{name}.txt")
    } else {
        format!("{name}.txt")
    }
}

/// Whether Windows takes `name`, with any extension, for a device rather
/// than a file: CON, PRN, AUX, NUL, COM1 to COM9 and LPT1 to LPT9. The
/// name is held to it on every system, so that a note is edited in a file
/// of the same name everywhere.
fn is_reserved_on_windows(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(name.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|device| {
            name.strip_prefix(device).is_some_and(|digit| {
                digit.len() == 1 && digit.chars().all(|c| ('1'..='9').contains(&c))
            })
        })
}

/// Opens `initial` in `editor`, in a file called `file_name`
/// inside a new folder that only this user can open, and returns what the
/// file holds when the editor exits. The folder is removed afterwards.
pub fn edit_in_editor(editor: &Editor, initial: &str, file_name: &str) -> Result<Edited, CliError> {
    let folder = tempfile::Builder::new().prefix("dirt-").tempdir()?;
    let file = folder.path().join(file_name);
    std::fs::write(&file, initial)?;

    let started = Instant::now();
    let launched = launch_editor(&editor.command, &file);
    let at_once = started.elapsed() < AT_ONCE;
    launched.map_err(|error| editor.failed(error))?;
    let text = std::fs::read_to_string(&file)?;
    Ok(Edited {
        text: normalize_content(&text),
        at_once,
    })
}

/// The editor to use, and where the choice came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    pub command: String,
    /// The variable that named it, or `None` for [`default_editor`].
    pub from: Option<&'static str>,
}

impl Editor {
    /// Says what went wrong, and for the editor used when none was set,
    /// how to set one.
    pub fn failed(&self, error: CliError) -> CliError {
        let why = match error {
            CliError::EditorFailed(why) => why,
            CliError::Io(error) => format!("`{}` could not be started ({error})", self.command),
            other => return other,
        };
        CliError::EditorFailed(self.from.map_or_else(
            || {
                format!(
                    "{why}. It is the editor used when neither VISUAL nor EDITOR is set. \
                     {SET_EDITOR}"
                )
            },
            |variable| format!("{why}. It is the editor {variable} names"),
        ))
    }
}

/// How to choose an editor.
pub const SET_EDITOR: &str = if cfg!(windows) {
    "Set EDITOR to the editor to use, as in `$env:EDITOR = \"code --wait\"` in PowerShell"
} else {
    "Set EDITOR to the editor to use, as in `export EDITOR=nano`"
};

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

/// `VISUAL`, else `EDITOR`, else [`default_editor`]. An empty variable is
/// as good as none.
pub fn preferred_editor() -> Editor {
    editor_from(env::var("VISUAL").ok(), env::var("EDITOR").ok())
}

/// [`preferred_editor`], given the two variables.
pub fn editor_from(visual: Option<String>, editor: Option<String>) -> Editor {
    let set = |value: Option<String>| value.filter(|value| !value.trim().is_empty());
    if let Some(command) = set(visual) {
        return Editor {
            command,
            from: Some("VISUAL"),
        };
    }
    if let Some(command) = set(editor) {
        return Editor {
            command,
            from: Some("EDITOR"),
        };
    }
    Editor {
        command: default_editor().to_owned(),
        from: None,
    }
}

pub const fn default_editor() -> &'static str {
    if cfg!(windows) {
        "notepad"
    } else {
        "vi"
    }
}
