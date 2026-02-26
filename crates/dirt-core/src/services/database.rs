//! Shared database service wrapper used across clients.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::db::{
    Database, LibSqlNoteRepository, LibSqlSettingsRepository, NoteRepository, SettingsRepository,
    SyncConfig,
};
use crate::models::{Attachment, AttachmentId, Note, Settings, SyncConflict};
use crate::{NoteId, Result};

/// Thread-safe service for DB and repository operations.
#[derive(Clone)]
pub struct DatabaseService {
    db: Arc<Mutex<Database>>,
    db_path: Option<PathBuf>,
    sync_config: Option<SyncConfig>,
}

impl DatabaseService {
    /// Open a database service at the given filesystem path.
    pub async fn open_path(
        db_path: impl Into<PathBuf>,
        sync_config: Option<SyncConfig>,
    ) -> Result<Self> {
        let db_path = db_path.into();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Self::open_database(db_path.clone(), sync_config.clone()).await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            db_path: Some(db_path),
            sync_config,
        })
    }

    /// Open a local-only database service at the given path.
    pub async fn open_local_path(db_path: impl Into<PathBuf>) -> Result<Self> {
        Self::open_path(db_path, None).await
    }

    /// Open a sync-enabled database service at the given path.
    pub async fn open_sync_path(
        db_path: impl Into<PathBuf>,
        sync_config: SyncConfig,
    ) -> Result<Self> {
        Self::open_path(db_path, Some(sync_config)).await
    }

    /// Open an in-memory database service (primarily for tests).
    pub async fn open_in_memory() -> Result<Self> {
        let db = Database::open_in_memory().await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            db_path: None,
            sync_config: None,
        })
    }

    async fn open_database(db_path: PathBuf, sync_config: Option<SyncConfig>) -> Result<Database> {
        if let Some(config) = sync_config {
            Self::open_database_with_sync_recovery(db_path, config)
        } else {
            tracing::info!("Running in local-only mode (no sync config)");
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
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| crate::Error::Database(error.to_string()))?
                    .block_on(Database::open_with_sync(&db_path, sync_config))
            })
            .map_err(|error| crate::Error::Database(error.to_string()))?
            .join()
            .map_err(|_| crate::Error::Database("Thread panicked".to_string()))?
    }

    fn is_corrupted_db_error(error: &crate::Error) -> bool {
        error
            .to_string()
            .to_ascii_lowercase()
            .contains("file is not a database")
    }

    fn is_recoverable_local_replica_error(error: &crate::Error) -> bool {
        if Self::is_corrupted_db_error(error) {
            return true;
        }

        let message = error.to_string().to_ascii_lowercase();
        message.contains("invalid local state")
            || message.contains("metadata file exists but db file does not")
    }

    fn quarantine_corrupted_db_files(db_path: &Path) -> Result<()> {
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

    /// Sync with remote DB when sync is enabled.
    pub async fn sync(&self) -> Result<()> {
        let db = self.db.lock().await;
        db.sync().await
    }

    /// Returns whether sync is configured for this DB.
    pub async fn is_sync_enabled(&self) -> bool {
        let db = self.db.lock().await;
        db.is_sync_enabled()
    }

    /// List notes newest-first.
    pub async fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list(limit, offset).await
    }

    /// Fetch a note by id.
    pub async fn get_note(&self, id: &NoteId) -> Result<Option<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.get(id).await
    }

    /// Create a new note.
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create(content).await
    }

    /// Create a note with a pre-generated id.
    pub async fn create_note_with_id(&self, note: &Note) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create_with_note(note).await
    }

    /// Update a note.
    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.update(id, content).await
    }

    /// Soft-delete a note.
    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.delete(id).await
    }

    /// Search notes by query.
    pub async fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.search(query, limit).await
    }

    /// List notes by tag.
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

    /// List tags and counts.
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

    /// Create attachment metadata for a note.
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

    /// List non-deleted attachment metadata for a note.
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

    /// Soft-delete attachment metadata by id.
    pub async fn delete_attachment(&self, attachment_id: &AttachmentId) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.delete_attachment(attachment_id).await
    }

    /// Load settings.
    pub async fn load_settings(&self) -> Result<Settings> {
        let db = self.db.lock().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.load().await
    }

    /// Save settings.
    pub async fn save_settings(&self, settings: &Settings) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.save(settings).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn in_memory_create_and_list_roundtrip() {
        let service = DatabaseService::open_in_memory().await.unwrap();

        service.create_note("hello core").await.unwrap();
        let notes = service.list_notes(10, 0).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "hello core");
    }

    #[test]
    fn detects_recoverable_local_replica_errors() {
        assert!(DatabaseService::is_recoverable_local_replica_error(
            &crate::Error::Database("SQLite failure: file is not a database".to_string())
        ));
        assert!(DatabaseService::is_recoverable_local_replica_error(
            &crate::Error::Database(
                "sync error: invalid local state: metadata file exists but db file does not"
                    .to_string()
            )
        ));
        assert!(!DatabaseService::is_recoverable_local_replica_error(
            &crate::Error::InvalidInput("note content cannot be empty".to_string())
        ));
    }

    #[test]
    fn quarantine_local_replica_files_moves_db_and_removes_sidecars() {
        let test_dir = std::env::temp_dir().join(format!(
            "dirt-core-recovery-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&test_dir).unwrap();

        let db_path = test_dir.join("dirt.db");
        let info_path = test_dir.join("dirt.db-info");
        let wal_path = test_dir.join("dirt.db-wal");

        std::fs::write(&db_path, b"bad-db").unwrap();
        std::fs::write(&info_path, b"meta").unwrap();
        std::fs::write(&wal_path, b"wal").unwrap();

        DatabaseService::quarantine_corrupted_db_files(&db_path).unwrap();

        assert!(!db_path.exists());
        assert!(!info_path.exists());
        assert!(!wal_path.exists());

        let mut found_backup = false;
        for entry in std::fs::read_dir(&test_dir).unwrap() {
            let entry = entry.unwrap();
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with("dirt.db.corrupt-") {
                found_backup = true;
                break;
            }
        }
        assert!(found_backup);

        let _ = std::fs::remove_dir_all(test_dir);
    }
}
