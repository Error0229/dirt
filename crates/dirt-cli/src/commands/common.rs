use std::env;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use dirt_core::services::db_paths::{read_active_user, resolve_active_db};
use dirt_core::services::DatabaseService;
use dirt_core::{Note, NoteId, SOLO_USER_ID};
use serde::Serialize;

use crate::error::CliError;

/// Resolved DB target for one CLI invocation.
///
/// Every command opens its DB via [`open_database`] using one of
/// these. The auth flow reaches `dirt_data_dir()` directly when it
/// needs to update `state.json` or run the legacy-solo migration —
/// the `DbScope` shape stays narrow because most commands only need
/// the (path, `user_id`) pair.
#[derive(Debug, Clone)]
pub struct DbScope {
    /// Absolute path to the `SQLite` file this command will open.
    pub path: PathBuf,
    /// Owner of the rows in that DB. New notes are stamped with this
    /// id; the sync engine reads it via `DatabaseService::user_id()`.
    pub user_id: String,
}

#[derive(Debug, Serialize)]
pub struct NoteListItem {
    pub id: String,
    pub preview: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub relative_time: String,
    pub tags: Vec<String>,
}

pub async fn list_notes(
    limit: usize,
    tag: Option<&str>,
    scope: &DbScope,
) -> Result<Vec<Note>, CliError> {
    let db = open_database(scope).await?;
    if let Some(tag_name) = tag {
        Ok(db.list_notes_by_tag(tag_name, limit, 0).await?)
    } else {
        Ok(db.list_notes(limit, 0).await?)
    }
}

pub async fn list_all_notes(scope: &DbScope) -> Result<Vec<Note>, CliError> {
    const PAGE_SIZE: usize = 500;

    let db = open_database(scope).await?;

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

pub async fn search_notes(
    query: &str,
    limit: usize,
    scope: &DbScope,
) -> Result<Vec<Note>, CliError> {
    let db = open_database(scope).await?;
    Ok(db.search_notes(query, limit).await?)
}

pub async fn resolve_note_for_edit(
    note_query: &str,
    db: &DatabaseService,
) -> Result<Note, CliError> {
    if let Ok(note_id) = note_query.parse::<NoteId>() {
        if let Some(note) = db.get_note(&note_id).await? {
            return Ok(note);
        }
    }

    let matching_ids = db.list_note_ids_by_prefix(note_query, 3).await?;

    match matching_ids.len() {
        0 => Err(CliError::NoteNotFound(note_query.to_string())),
        1 => {
            let resolved_id = matching_ids[0]
                .parse::<NoteId>()
                .map_err(|_| CliError::NoteNotFound(note_query.to_string()))?;
            db.get_note(&resolved_id)
                .await?
                .ok_or_else(|| CliError::NoteNotFound(note_query.to_string()))
        }
        _ => {
            let options = matching_ids
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
            let tags = render_tags(note);

            if tags.is_empty() {
                format!("{short_id:<13}  {preview:<40}  {relative_time}")
            } else {
                format!("{short_id:<13}  {preview:<40}  {relative_time:<10}  {tags}")
            }
        })
        .collect()
}

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
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
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

/// Resolve which DB this CLI invocation should open and what `user_id`
/// to stamp new notes with.
///
/// Resolution order (matches the active-user-pointer design in
/// `docs/plans/2026-05-26-issue-234-per-user-db-partitioning.md`):
///
/// 1. **`--db-path` flag** or **`DIRT_DB_PATH` env var** → honor the
///    path verbatim and look up `user_id` from `state.json` if it
///    exists in the *default* `<os_data>/dirt` directory, else fall
///    back to `SOLO_USER_ID`. `data_dir` is set to `None` so the
///    auth flow won't write state into a test override.
/// 2. **`state.json` at `<data_dir>/state.json`** → return
///    `(<data_dir>/<user_id>/dirt.db, <user_id>)`.
/// 3. **No state file** → legacy single-DB layout
///    `(<data_dir>/dirt.db, SOLO_USER_ID)`. Only reachable on a
///    machine that has never signed in.
///
/// Tests can override the dirt data dir via `DIRT_DATA_DIR`.
pub async fn resolve_db_scope(cli_db_path: Option<PathBuf>) -> Result<DbScope, CliError> {
    let default_data_dir = dirt_data_dir();
    let explicit = cli_db_path.or_else(|| env::var_os("DIRT_DB_PATH").map(PathBuf::from));
    if let Some(path) = explicit {
        // Explicit override: respect the path but still pick up the
        // active user_id (if any) so a one-off `--db-path` invocation
        // doesn't silently stamp new notes with the wrong tenant.
        let user_id = match read_active_user(&default_data_dir).await {
            Ok(Some(uid)) => uid,
            _ => SOLO_USER_ID.to_string(),
        };
        return Ok(DbScope { path, user_id });
    }
    let (path, user_id) = resolve_active_db(&default_data_dir).await?;
    Ok(DbScope { path, user_id })
}

/// Canonical `<os_data>/dirt` directory, with a `DIRT_DATA_DIR` env
/// override for tests. Pub-in-crate so the auth command can write
/// `state.json` against the same root.
#[must_use]
pub fn dirt_data_dir() -> PathBuf {
    if let Some(override_dir) = env::var_os("DIRT_DATA_DIR") {
        return PathBuf::from(override_dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| panic!("Failed to resolve CLI data directory"))
        .join("dirt")
}

/// Open the DB referenced by `scope`. Wraps
/// [`DatabaseService::open_for_user`] so callers don't have to
/// re-thread the `user_id` argument at every call site.
pub async fn open_database(scope: &DbScope) -> Result<DatabaseService, CliError> {
    if let Some(parent) = scope.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(DatabaseService::open_for_user(scope.path.clone(), scope.user_id.clone()).await?)
}
