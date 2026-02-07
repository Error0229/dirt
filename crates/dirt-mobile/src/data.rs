//! Data access layer for the mobile app.

#[cfg(target_os = "android")]
use std::path::PathBuf;
use std::sync::Arc;

use dirt_core::db::{Database, LibSqlNoteRepository, NoteRepository};
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

        let db = Database::open(db_path).await?;
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
}
