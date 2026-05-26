//! Data access layer for the mobile app.

#[cfg(target_os = "android")]
use std::path::PathBuf;

use std::ops::Deref;

use dirt_core::models::{Note, NoteId};
use dirt_core::services::DatabaseService as CoreDatabaseService;
use dirt_core::{Error, Result};

#[cfg(target_os = "android")]
use crate::config::default_mobile_data_directory;

const DEFAULT_NOTES_LIMIT: usize = 100;
const EXPORT_NOTES_PAGE_SIZE: usize = 500;

/// Thin async wrapper around shared core database service APIs.
///
/// Derefs to the inner `CoreDatabaseService` so that consumers like
/// `SyncEngine::new(&store, ..)` work without an explicit getter — the
/// engine wants the core type, and the mobile-specific surface here is
/// just sugar over it.
#[derive(Clone)]
pub struct MobileNoteStore {
    db: CoreDatabaseService,
}

impl Deref for MobileNoteStore {
    type Target = CoreDatabaseService;

    fn deref(&self) -> &Self::Target {
        &self.db
    }
}

impl MobileNoteStore {
    /// Open the per-user DB at `<data_dir>/<user_id>/dirt-mobile.db`.
    #[cfg(target_os = "android")]
    pub async fn open_for_user(user_id: &str) -> Result<Self> {
        let db_path = user_db_path_for(user_id)?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = CoreDatabaseService::open_for_user(db_path, user_id).await?;
        Ok(Self { db })
    }

    /// Open the legacy pre-signin solo mobile DB. Only reachable on a
    /// brand-new install that has never signed in.
    #[cfg(target_os = "android")]
    pub async fn open_solo() -> Result<Self> {
        let db_path = solo_db_path_mobile();
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

/// Pre-signin legacy DB path used only on a brand-new install. After
/// the first sign-in the migration moves this file under the user's
/// directory and this location is never re-created. Delegates to
/// `dirt_core::services::db_paths::solo_db_path` with the mobile
/// filename so the migration helper finds the right file on disk.
#[cfg(target_os = "android")]
fn solo_db_path_mobile() -> PathBuf {
    dirt_core::services::db_paths::solo_db_path(
        &default_mobile_data_directory(),
        dirt_core::services::db_paths::MOBILE_DB_FILENAME,
    )
}

/// Per-user DB path for `user_id` under the mobile data directory.
/// Delegates to the core helper so the layout is identical to
/// desktop / CLI aside from the filename.
#[cfg(target_os = "android")]
fn user_db_path_for(user_id: &str) -> Result<PathBuf> {
    dirt_core::services::db_paths::user_db_path(
        &default_mobile_data_directory(),
        user_id,
        dirt_core::services::db_paths::MOBILE_DB_FILENAME,
    )
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
