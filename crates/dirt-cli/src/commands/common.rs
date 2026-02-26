use std::env;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use dirt_core::db::{Database, LibSqlNoteRepository, NoteRepository, SyncConfig};
use dirt_core::{Note, NoteId, SyncConflict};
use serde::Serialize;

use crate::auth::{clear_stored_session, load_stored_session, SupabaseAuthService};
use crate::config_profiles::CliProfilesConfig;
use crate::error::CliError;
use crate::managed_sync::ManagedSyncAuthClient;

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

#[derive(Debug, Serialize)]
pub struct SyncConflictItem {
    pub id: i64,
    pub note_id: String,
    pub local_updated_at: i64,
    pub incoming_updated_at: i64,
    pub resolved_at: i64,
    pub resolved_at_iso: String,
    pub strategy: String,
}

pub async fn list_notes(
    limit: usize,
    tag: Option<&str>,
    db_path: &Path,
) -> Result<Vec<Note>, CliError> {
    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());

    if let Some(tag_name) = tag {
        Ok(repo.list_by_tag(tag_name, limit, 0).await?)
    } else {
        Ok(repo.list(limit, 0).await?)
    }
}

pub async fn list_all_notes(db_path: &Path) -> Result<Vec<Note>, CliError> {
    const PAGE_SIZE: usize = 500;

    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());

    let mut notes = Vec::new();
    let mut offset = 0usize;

    loop {
        let batch = repo.list(PAGE_SIZE, offset).await?;
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
    db_path: &Path,
) -> Result<Vec<Note>, CliError> {
    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());
    Ok(repo.search(query, limit).await?)
}

pub async fn list_sync_conflicts(
    limit: usize,
    db_path: &Path,
) -> Result<Vec<SyncConflict>, CliError> {
    let db = open_database(db_path).await?;
    let repo = LibSqlNoteRepository::new(db.connection());
    Ok(repo.list_conflicts(limit).await?)
}

pub async fn resolve_note_for_edit(note_query: &str, db: &Database) -> Result<Note, CliError> {
    let repo = LibSqlNoteRepository::new(db.connection());

    if let Ok(note_id) = note_query.parse::<NoteId>() {
        if let Some(note) = repo.get(&note_id).await? {
            return Ok(note);
        }
    }

    let mut rows = db
        .connection()
        .query(
            "SELECT id
             FROM notes
             WHERE is_deleted = 0 AND id LIKE ?
             ORDER BY updated_at DESC
             LIMIT ?",
            libsql::params![format!("{note_query}%"), 3i64],
        )
        .await?;

    let mut matching_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        matching_ids.push(id);
    }

    match matching_ids.len() {
        0 => Err(CliError::NoteNotFound(note_query.to_string())),
        1 => {
            let resolved_id = matching_ids[0]
                .parse::<NoteId>()
                .map_err(|_| CliError::NoteNotFound(note_query.to_string()))?;
            repo.get(&resolved_id)
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

pub fn sync_conflict_to_item(conflict: &SyncConflict) -> SyncConflictItem {
    SyncConflictItem {
        id: conflict.id,
        note_id: conflict.note_id.clone(),
        local_updated_at: conflict.local_updated_at,
        incoming_updated_at: conflict.incoming_updated_at,
        resolved_at: conflict.resolved_at,
        resolved_at_iso: format_sync_timestamp(conflict.resolved_at),
        strategy: conflict.strategy.clone(),
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

pub fn format_sync_conflict_lines(conflicts: &[SyncConflict]) -> Vec<String> {
    conflicts
        .iter()
        .map(|conflict| {
            format!(
                "{}  {:<4}  note={}  local={} incoming={}",
                format_sync_timestamp(conflict.resolved_at),
                conflict.strategy,
                conflict.note_id,
                conflict.local_updated_at,
                conflict.incoming_updated_at
            )
        })
        .collect()
}

pub fn format_sync_timestamp(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms).map_or_else(
        || timestamp_ms.to_string(),
        |date_time| date_time.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
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

pub fn resolve_db_path(cli_db_path: Option<PathBuf>) -> PathBuf {
    cli_db_path
        .or_else(|| env::var_os("DIRT_DB_PATH").map(PathBuf::from))
        .unwrap_or_else(default_db_path)
}

pub fn default_db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dirt")
        .join("dirt.db")
}

#[derive(Clone, Copy)]
enum OpenDatabaseMode {
    Standard,
    RequireSync,
}

impl OpenDatabaseMode {
    const fn requires_sync(self) -> bool {
        matches!(self, Self::RequireSync)
    }
}

pub async fn open_database(path: &Path) -> Result<Database, CliError> {
    open_database_with_mode(path, OpenDatabaseMode::Standard).await
}

pub async fn open_sync_database(path: &Path) -> Result<Database, CliError> {
    open_database_with_mode(path, OpenDatabaseMode::RequireSync).await
}

async fn open_database_with_mode(
    path: &Path,
    mode: OpenDatabaseMode,
) -> Result<Database, CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let sync_config = sync_config_from_profile(mode).await?;

    if let Some(sync_config) = sync_config {
        let path_buf = path.to_path_buf();
        let db = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| dirt_core::Error::Database(error.to_string()))?;
                runtime.block_on(Database::open_with_sync(&path_buf, sync_config))
            })
            .map_err(|error| CliError::DatabaseInit(error.to_string()))?
            .join()
            .map_err(|_| CliError::DatabaseInit("sync initialization thread panicked".into()))??;

        Ok(db)
    } else {
        Ok(Database::open(path).await?)
    }
}

async fn sync_config_from_profile(mode: OpenDatabaseMode) -> Result<Option<SyncConfig>, CliError> {
    let config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(None);
    let Some(profile) = config.profile(&profile_name) else {
        return Ok(None);
    };
    let Some(endpoint) = profile.managed_sync_endpoint() else {
        return Ok(None);
    };

    let maybe_auth_service = SupabaseAuthService::new_for_profile(&profile_name, profile)
        .map_err(|error| CliError::Auth(error.to_string()))?;
    let mut session = if let Some(service) = maybe_auth_service.as_ref() {
        service
            .restore_session()
            .await
            .map_err(|error| CliError::Auth(error.to_string()))?
    } else {
        load_stored_session(&profile_name).map_err(|error| CliError::Auth(error.to_string()))?
    };

    if let Some(stored) = session.as_ref() {
        if stored.is_expired() {
            if let Some(service) = maybe_auth_service {
                session = service
                    .refresh_session(&stored.refresh_token)
                    .await
                    .map(Some)
                    .map_err(|error| CliError::Auth(error.to_string()))?;
            } else {
                clear_stored_session(&profile_name)
                    .map_err(|error| CliError::Auth(error.to_string()))?;
                session = None;
            }
        }
    }

    let Some(session) = session else {
        if mode.requires_sync() {
            return Err(CliError::SyncNotConfigured);
        }
        return Ok(None);
    };

    let sync_auth_client = ManagedSyncAuthClient::new(endpoint)
        .map_err(|error| CliError::ManagedSync(error.to_string()))?;
    let managed_token = match sync_auth_client.exchange_token(&session.access_token).await {
        Ok(token) => token,
        Err(error) => {
            if mode.requires_sync() {
                return Err(CliError::ManagedSync(error.to_string()));
            }
            tracing::warn!(
                "Managed sync token exchange failed for profile '{}': {}",
                profile_name,
                error
            );
            return Ok(None);
        }
    };

    tracing::info!("Managed sync enabled via profile '{}'", profile_name);
    Ok(Some(SyncConfig::new(
        managed_token.database_url,
        managed_token.token,
    )))
}
