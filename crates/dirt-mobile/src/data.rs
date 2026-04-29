//! Data access layer for the mobile app.

#[cfg(target_os = "android")]
use std::path::PathBuf;

use dirt_core::models::{Note, NoteId};
use dirt_core::services::DatabaseService as CoreDatabaseService;
use dirt_core::{Error, Result};

#[cfg(target_os = "android")]
use crate::config::default_mobile_data_directory;

const DEFAULT_NOTES_LIMIT: usize = 100;
const EXPORT_NOTES_PAGE_SIZE: usize = 500;

/// Thin async wrapper around shared core database service APIs.
#[derive(Clone)]
pub struct MobileNoteStore {
    db: CoreDatabaseService,
}

impl MobileNoteStore {
    /// Open the default local mobile database path.
    #[cfg(target_os = "android")]
    pub async fn open_default() -> Result<Self> {
        let db_path = default_db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = CoreDatabaseService::open_local_path(db_path).await?;
        Ok(Self { db })
    }

    /// Open an in-memory database for tests.
    #[cfg(test)]
    pub async fn open_in_memory() -> Result<Self> {
        let db = CoreDatabaseService::open_in_memory().await?;
        Ok(Self { db })
    }

    /// List notes newest-first.
    pub async fn list_notes(&self) -> Result<Vec<Note>> {
        self.db.list_notes(DEFAULT_NOTES_LIMIT, 0).await
    }

    /// List all notes for full export operations.
    pub async fn list_all_notes(&self) -> Result<Vec<Note>> {
        let mut notes = Vec::new();
        let mut offset = 0usize;

        loop {
            let batch = self.db.list_notes(EXPORT_NOTES_PAGE_SIZE, offset).await?;
            let count = batch.len();
            notes.extend(batch);

            if count < EXPORT_NOTES_PAGE_SIZE {
                break;
            }
            offset += count;
        }

        Ok(notes)
    }

    /// Create a note.
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let normalized = normalize_content(content)?;
        self.db.create_note(&normalized).await
    }

    /// Update an existing note.
    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let normalized = normalize_content(content)?;
        self.db.update_note(id, &normalized).await
    }

    /// Soft delete a note.
    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        self.db.delete_note(id).await
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
    default_mobile_data_directory().join("dirt-mobile.db")
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

    #[tokio::test(flavor = "multi_thread")]
    async fn list_all_notes_returns_full_collection() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();
        store.create_note("One").await.unwrap();
        store.create_note("Two").await.unwrap();
        store.create_note("Three").await.unwrap();

        let notes = store.list_all_notes().await.unwrap();
        assert_eq!(notes.len(), 3);
    }
}
