//! Data access layer for the mobile app.

#[cfg(target_os = "android")]
use std::path::PathBuf;
use std::sync::Arc;

use dirt_core::db::{Database, LibSqlNoteRepository, NoteRepository, SyncConfig};
use dirt_core::models::{Note, NoteId};
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
            Database::open_with_sync(db_path, sync_config).await?
        } else {
            Database::open(db_path).await?
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
}
