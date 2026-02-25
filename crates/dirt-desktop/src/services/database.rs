//! Database service for the desktop application

#![allow(dead_code)] // Methods will be used in future features

use std::path::PathBuf;
use std::sync::Arc;

use dirt_core::db::{
    Database, LibSqlNoteRepository, LibSqlSettingsRepository, NoteRepository, SettingsRepository,
    SyncConfig,
};
use dirt_core::error::Result;
use dirt_core::models::{Attachment, AttachmentId, Note, Settings, SyncConflict};
use dirt_core::NoteId;
use tokio::sync::Mutex;

/// Service for database operations
///
/// Wraps the database connection and provides thread-safe async access.
#[derive(Clone)]
pub struct DatabaseService {
    db: Arc<Mutex<Database>>,
    db_path: Option<PathBuf>,
    sync_config: Option<SyncConfig>,
}

impl DatabaseService {
    /// Create a new local-only database service.
    pub async fn new() -> Result<Self> {
        let db_path = Self::default_db_path();

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Self::open_database(db_path.clone(), None).await?;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            db_path: Some(db_path),
            sync_config: None,
        })
    }

    /// Create a new database service with explicit sync config
    pub async fn new_with_sync(sync_config: SyncConfig) -> Result<Self> {
        let db_path = Self::default_db_path();

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Self::open_database(db_path.clone(), Some(sync_config.clone())).await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            db_path: Some(db_path),
            sync_config: Some(sync_config),
        })
    }

    /// Create an in-memory database service (for testing)
    #[allow(dead_code)]
    pub async fn in_memory() -> Result<Self> {
        let db = Database::open_in_memory().await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            db_path: None,
            sync_config: None,
        })
    }

    /// Open a local DB or embedded replica using the same initialization strategy
    /// as app startup.
    async fn open_database(db_path: PathBuf, sync_config: Option<SyncConfig>) -> Result<Database> {
        if let Some(config) = sync_config {
            Self::open_database_with_sync_recovery(db_path, config)
        } else {
            tracing::info!("Running in local-only mode (no TURSO_DATABASE_URL/TURSO_AUTH_TOKEN)");
            Database::open(&db_path).await
        }
    }

    fn open_database_with_sync_recovery(
        db_path: PathBuf,
        sync_config: SyncConfig,
    ) -> Result<Database> {
        tracing::info!(
            "Sync enabled with Turso: {}",
            sync_config.url.as_deref().unwrap_or("unknown")
        );
        match Self::open_database_with_sync_thread(db_path.clone(), sync_config.clone()) {
            Ok(db) => Ok(db),
            Err(error) if Self::is_recoverable_local_replica_error(&error) => {
                tracing::warn!(
                    "Detected inconsistent local replica state at {}: {}. Resetting local replica files and retrying once.",
                    db_path.display(),
                    error
                );
                Self::quarantine_corrupted_db_files(&db_path)?;
                Self::open_database_with_sync_thread(db_path, sync_config)
            }
            Err(error) => Err(error),
        }
    }

    fn open_database_with_sync_thread(
        db_path: PathBuf,
        sync_config: SyncConfig,
    ) -> Result<Database> {
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024) // 8MB stack
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(Database::open_with_sync(&db_path, sync_config))
            })
            .map_err(|error| dirt_core::error::Error::Database(error.to_string()))?
            .join()
            .map_err(|_| dirt_core::error::Error::Database("Thread panicked".to_string()))?
    }

    fn is_corrupted_db_error(error: &dirt_core::Error) -> bool {
        error
            .to_string()
            .to_ascii_lowercase()
            .contains("file is not a database")
    }

    fn is_recoverable_local_replica_error(error: &dirt_core::Error) -> bool {
        if Self::is_corrupted_db_error(error) {
            return true;
        }

        let message = error.to_string().to_ascii_lowercase();
        message.contains("invalid local state")
            || message.contains("metadata file exists but db file does not")
    }

    fn quarantine_corrupted_db_files(db_path: &PathBuf) -> Result<()> {
        if db_path.exists() {
            let timestamp = chrono::Utc::now().timestamp_millis();
            let backup_name = format!("dirt.db.corrupt-{timestamp}");
            let backup_path = db_path.with_file_name(backup_name);

            std::fs::rename(db_path, &backup_path)?;
            tracing::warn!(
                "Moved corrupted local DB file from {} to {}",
                db_path.display(),
                backup_path.display()
            );
        }

        let Some(parent) = db_path.parent() else {
            return Ok(());
        };
        let Some(base_name) = db_path.file_name().and_then(|name| name.to_str()) else {
            return Ok(());
        };
        let sidecar_prefix = format!("{base_name}-");

        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with(&sidecar_prefix) {
                let path = entry.path();
                std::fs::remove_file(&path)?;
                tracing::warn!("Removed stale local replica file {}", path.display());
            }
        }

        Ok(())
    }

    async fn reopen_after_corruption(&self) -> Result<bool> {
        let Some(db_path) = self.db_path.clone() else {
            return Ok(false);
        };

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        tracing::warn!(
            "Detected invalid local DB file; attempting to reopen connection at {}",
            db_path.display()
        );

        // Replace with an in-memory placeholder first so the old file handle is released on
        // Windows before we move corrupted files out of the way.
        {
            let mut db = self.db.lock().await;
            let placeholder = Database::open_in_memory().await?;
            let _old = std::mem::replace(&mut *db, placeholder);
        }

        Self::quarantine_corrupted_db_files(&db_path)?;
        let reopened = Self::open_database(db_path, self.sync_config.clone()).await?;
        let mut db = self.db.lock().await;
        *db = reopened;
        Ok(true)
    }

    /// Get the default database path
    fn default_db_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dirt")
            .join("dirt.db")
    }

    /// Sync with remote database (if configured)
    pub async fn sync(&self) -> Result<()> {
        let db = self.db.lock().await;
        db.sync().await
    }

    /// Check if sync is enabled
    pub async fn is_sync_enabled(&self) -> bool {
        let db = self.db.lock().await;
        db.is_sync_enabled()
    }

    /// List all notes
    pub async fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list(limit, offset).await
    }

    /// Get a note by ID
    pub async fn get_note(&self, id: &NoteId) -> Result<Option<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.get(id).await
    }

    /// Create a new note
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create(content).await
    }

    /// Create a note with a pre-generated ID (for optimistic UI updates)
    pub async fn create_note_with_id(&self, note: &Note) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create_with_note(note).await
    }

    /// Update a note
    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.update(id, content).await
    }

    /// Delete a note (soft delete)
    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.delete(id).await
    }

    /// Search notes
    pub async fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.search(query, limit).await
    }

    /// List notes by tag
    pub async fn list_notes_by_tag(
        &self,
        tag: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_by_tag(tag, limit, offset).await
    }

    /// Get all tags with counts
    pub async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_tags().await
    }

    /// List recently resolved sync conflicts.
    pub async fn list_conflicts(&self, limit: usize) -> Result<Vec<SyncConflict>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_conflicts(limit).await
    }

    /// Create attachment metadata for a note
    pub async fn create_attachment(
        &self,
        note_id: &NoteId,
        filename: &str,
        mime_type: &str,
        size_bytes: i64,
        r2_key: &str,
    ) -> Result<Attachment> {
        let first_attempt = {
            let db = self.db.lock().await;
            let repo = LibSqlNoteRepository::new(db.connection());
            repo.create_attachment(note_id, filename, mime_type, size_bytes, r2_key)
                .await
        };

        match first_attempt {
            Ok(attachment) => Ok(attachment),
            Err(error) if Self::is_corrupted_db_error(&error) => {
                if self.reopen_after_corruption().await? {
                    let db = self.db.lock().await;
                    let repo = LibSqlNoteRepository::new(db.connection());
                    repo.create_attachment(note_id, filename, mime_type, size_bytes, r2_key)
                        .await
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    /// List non-deleted attachment metadata for a note
    pub async fn list_attachments(&self, note_id: &NoteId) -> Result<Vec<Attachment>> {
        let first_attempt = {
            let db = self.db.lock().await;
            let repo = LibSqlNoteRepository::new(db.connection());
            repo.list_attachments(note_id).await
        };

        match first_attempt {
            Ok(attachments) => Ok(attachments),
            Err(error) if Self::is_corrupted_db_error(&error) => {
                if self.reopen_after_corruption().await? {
                    let db = self.db.lock().await;
                    let repo = LibSqlNoteRepository::new(db.connection());
                    repo.list_attachments(note_id).await
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    /// Soft delete attachment metadata by id
    pub async fn delete_attachment(&self, attachment_id: &AttachmentId) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.delete_attachment(attachment_id).await
    }

    /// Load settings from database
    pub async fn load_settings(&self) -> Result<Settings> {
        let db = self.db.lock().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.load().await
    }

    /// Save settings to database
    pub async fn save_settings(&self, settings: &Settings) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.save(settings).await
    }
}
