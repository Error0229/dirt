//! Data access layer for the mobile app.

#[cfg(target_os = "android")]
use std::path::PathBuf;

use dirt_core::models::{Note, NoteId};
#[cfg(target_os = "android")]
use dirt_core::store::{store_path, StoreName};
use dirt_core::{Error, Result, Store};

#[cfg(target_os = "android")]
use crate::config::default_mobile_data_directory;

const DEFAULT_NOTES_LIMIT: usize = 100;
const EXPORT_NOTES_PAGE_SIZE: usize = 500;

/// Thin wrapper around the shared core [`Store`] that adds the mobile
/// app's defaults: where the database lives, how many notes the list
/// shows, and trimming of note content.
#[derive(Clone)]
pub struct MobileNoteStore {
    db: Store,
}

impl MobileNoteStore {
    /// Open the local store at the default mobile database path.
    #[cfg(target_os = "android")]
    pub fn open_default() -> Result<Self> {
        let db = Store::open(default_db_path())?;
        Ok(Self { db })
    }

    /// Open a store on a file inside a new temporary folder, for tests.
    /// The folder is removed when the returned guard is dropped, so the
    /// caller keeps the guard for as long as it uses the store.
    #[cfg(test)]
    pub fn open_temporary() -> (tempfile::TempDir, Self) {
        let folder = tempfile::tempdir().unwrap();
        let db = Store::open(folder.path().join("notes.db")).unwrap();
        (folder, Self { db })
    }

    /// List notes, most recently edited first.
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

    /// Delete a note.
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

/// The database file of the local store under the mobile data directory.
#[cfg(target_os = "android")]
pub fn default_db_path() -> PathBuf {
    store_path(&default_mobile_data_directory(), &StoreName::Local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn create_update_delete_roundtrip() {
        let (_folder, store) = MobileNoteStore::open_temporary();

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
        let (_folder, store) = MobileNoteStore::open_temporary();
        let err = store.create_note("   ").await.unwrap_err();

        match err {
            Error::InvalidInput(msg) => assert!(msg.contains("cannot be empty")),
            other => panic!("expected invalid input error, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_all_notes_returns_full_collection() {
        let (_folder, store) = MobileNoteStore::open_temporary();
        store.create_note("One").await.unwrap();
        store.create_note("Two").await.unwrap();
        store.create_note("Three").await.unwrap();

        let notes = store.list_all_notes().await.unwrap();
        assert_eq!(notes.len(), 3);
    }
}
