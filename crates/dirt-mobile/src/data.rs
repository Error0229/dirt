//! Data access layer for the mobile app.

#[cfg(any(target_os = "android", test))]
use std::path::Path;
#[cfg(target_os = "android")]
use std::path::PathBuf;
use std::sync::Arc;

use dirt_core::db::{Database, LibSqlNoteRepository, NoteRepository, SyncConfig};
use dirt_core::models::{Attachment, Note, NoteId};
use dirt_core::{Error, Result};
use tokio::sync::Mutex;

const DEFAULT_NOTES_LIMIT: usize = 100;

/// Thin async wrapper around `dirt-core` note repository APIs.
#[derive(Clone)]
pub struct MobileNoteStore {
    db: Arc<Mutex<Database>>,
}

impl MobileNoteStore {
    /// Open the default local mobile database path.
    #[cfg(target_os = "android")]
    pub async fn open_default() -> Result<Self> {
        let db_path = default_db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = if let Some(sync_config) = sync_config_from_env() {
            open_with_sync_recovery(&db_path, sync_config).await?
        } else {
            Database::open(&db_path).await?
        };
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }

    /// Open an in-memory database for tests.
    #[cfg(test)]
    pub async fn open_in_memory() -> Result<Self> {
        let db = Database::open_in_memory().await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }

    /// List notes newest-first.
    pub async fn list_notes(&self) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list(DEFAULT_NOTES_LIMIT, 0).await
    }

    /// Create a note.
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let normalized = normalize_content(content)?;
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create(&normalized).await
    }

    /// Update an existing note.
    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let normalized = normalize_content(content)?;
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.update(id, &normalized).await
    }

    /// Soft delete a note.
    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.delete(id).await
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
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create_attachment(note_id, filename, mime_type, size_bytes, r2_key)
            .await
    }

    /// List attachment metadata for a note.
    pub async fn list_attachments(&self, note_id: &NoteId) -> Result<Vec<Attachment>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_attachments(note_id).await
    }

    /// Sync with remote database (if configured).
    pub async fn sync(&self) -> Result<()> {
        let db = self.db.lock().await;
        db.sync().await
    }

    /// Check whether remote sync is enabled.
    pub async fn is_sync_enabled(&self) -> bool {
        let db = self.db.lock().await;
        db.is_sync_enabled()
    }
}

fn normalize_content(content: &str) -> Result<String> {
    let normalized = content.trim();
    if normalized.is_empty() {
        return Err(Error::InvalidInput(
            "Note content cannot be empty".to_string(),
        ));
    }
    Ok(normalized.to_string())
}

#[cfg(target_os = "android")]
fn sync_config_from_env() -> Option<SyncConfig> {
    parse_sync_config(
        std::env::var("TURSO_DATABASE_URL").ok(),
        std::env::var("TURSO_AUTH_TOKEN").ok(),
    )
}

fn parse_sync_config(url: Option<String>, auth_token: Option<String>) -> Option<SyncConfig> {
    let url = url?.trim().to_string();
    let auth_token = auth_token?.trim().to_string();

    if url.is_empty() || auth_token.is_empty() {
        return None;
    }

    Some(SyncConfig::new(url, auth_token))
}

#[cfg(target_os = "android")]
async fn open_with_sync_recovery(db_path: &Path, sync_config: SyncConfig) -> Result<Database> {
    match Database::open_with_sync(db_path, sync_config.clone()).await {
        Ok(db) => Ok(db),
        Err(error) if is_recoverable_local_replica_error(&error) => {
            tracing::warn!(
                "Detected inconsistent local mobile replica at {}: {}. Resetting local replica files and retrying once.",
                db_path.display(),
                error
            );
            quarantine_local_replica_files(db_path)?;
            Database::open_with_sync(db_path, sync_config).await
        }
        Err(error) => Err(error),
    }
}

#[cfg(any(target_os = "android", test))]
fn is_recoverable_local_replica_error(error: &Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("file is not a database")
        || message.contains("invalid local state")
        || message.contains("metadata file exists but db file does not")
}

#[cfg(any(target_os = "android", test))]
fn quarantine_local_replica_files(db_path: &Path) -> Result<()> {
    if db_path.exists() {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let base_name = db_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("dirt-mobile.db");
        let backup_path = db_path.with_file_name(format!("{base_name}.corrupt-{timestamp}"));

        std::fs::rename(db_path, &backup_path)?;
        tracing::warn!(
            "Moved corrupted mobile DB file from {} to {}",
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
            tracing::warn!("Removed stale mobile replica file {}", path.display());
        }
    }

    Ok(())
}

/// Build a mobile-friendly local DB path.
#[cfg(target_os = "android")]
pub fn default_db_path() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dirt")
        .join("dirt-mobile.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn create_update_delete_roundtrip() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();

        let created = store.create_note("  Hello mobile  ").await.unwrap();
        assert_eq!(created.content, "Hello mobile");

        let updated = store
            .update_note(&created.id, "Updated #mobile")
            .await
            .unwrap();
        assert_eq!(updated.content, "Updated #mobile");
        assert_eq!(updated.id, created.id);

        store.delete_note(&updated.id).await.unwrap();
        let notes = store.list_notes().await.unwrap();
        assert!(notes.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_rejects_empty_content() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();
        let err = store.create_note("   ").await.unwrap_err();

        match err {
            Error::InvalidInput(msg) => assert!(msg.contains("cannot be empty")),
            other => panic!("expected invalid input error, got {other:?}"),
        }
    }

    #[test]
    fn parse_sync_config_requires_both_values() {
        assert!(parse_sync_config(None, Some("token".to_string())).is_none());
        assert!(parse_sync_config(Some("libsql://db.turso.io".to_string()), None).is_none());
    }

    #[test]
    fn parse_sync_config_rejects_empty_values() {
        assert!(parse_sync_config(Some("   ".to_string()), Some("token".to_string())).is_none());
        assert!(parse_sync_config(
            Some("libsql://db.turso.io".to_string()),
            Some("   ".to_string())
        )
        .is_none());
    }

    #[test]
    fn parse_sync_config_accepts_valid_values() {
        let config = parse_sync_config(
            Some(" libsql://db.turso.io ".to_string()),
            Some(" token ".to_string()),
        )
        .unwrap();

        assert_eq!(config.url.as_deref(), Some("libsql://db.turso.io"));
        assert_eq!(config.auth_token.as_deref(), Some("token"));
        assert!(config.is_configured());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn in_memory_store_sync_is_disabled() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();
        assert!(!store.is_sync_enabled().await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn in_memory_store_sync_is_noop() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();
        store.sync().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn attachment_metadata_roundtrip() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();
        let note = store.create_note("Attachment host note").await.unwrap();

        let created = store
            .create_attachment(
                &note.id,
                "mobile-photo.jpg",
                "image/jpeg",
                4242,
                "notes/mobile/mobile-photo.jpg",
            )
            .await
            .unwrap();

        let attachments = store.list_attachments(&note.id).await.unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].id, created.id);
        assert_eq!(attachments[0].r2_key, "notes/mobile/mobile-photo.jpg");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_attachment_requires_existing_note() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();

        let missing_note = NoteId::new();
        let err = store
            .create_attachment(
                &missing_note,
                "missing.png",
                "image/png",
                1,
                "notes/missing.png",
            )
            .await
            .unwrap_err();

        match err {
            Error::NotFound(value) => assert_eq!(value, missing_note.to_string()),
            other => panic!("expected not found, got {other:?}"),
        }
    }

    #[test]
    fn detects_recoverable_local_replica_errors() {
        assert!(is_recoverable_local_replica_error(&Error::Database(
            "SQLite failure: file is not a database".to_string()
        )));
        assert!(is_recoverable_local_replica_error(&Error::Database(
            "sync error: invalid local state: metadata file exists but db file does not"
                .to_string()
        )));
        assert!(!is_recoverable_local_replica_error(&Error::InvalidInput(
            "note content cannot be empty".to_string()
        )));
    }

    #[test]
    fn quarantine_local_replica_files_moves_db_and_removes_sidecars() {
        let test_dir = std::env::temp_dir().join(format!(
            "dirt-mobile-recovery-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&test_dir).unwrap();

        let db_path = test_dir.join("dirt-mobile.db");
        let info_path = test_dir.join("dirt-mobile.db-info");
        let wal_path = test_dir.join("dirt-mobile.db-wal");

        std::fs::write(&db_path, b"bad-db").unwrap();
        std::fs::write(&info_path, b"meta").unwrap();
        std::fs::write(&wal_path, b"wal").unwrap();

        quarantine_local_replica_files(&db_path).unwrap();

        assert!(!db_path.exists());
        assert!(!info_path.exists());
        assert!(!wal_path.exists());

        let mut found_backup = false;
        for entry in std::fs::read_dir(&test_dir).unwrap() {
            let entry = entry.unwrap();
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with("dirt-mobile.db.corrupt-") {
                found_backup = true;
                break;
            }
        }
        assert!(found_backup);

        let _ = std::fs::remove_dir_all(test_dir);
    }
}
