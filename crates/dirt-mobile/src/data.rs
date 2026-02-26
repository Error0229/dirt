//! Data access layer for the mobile app.

#[cfg(target_os = "android")]
use std::path::PathBuf;

use dirt_core::models::{Attachment, AttachmentId, Note, NoteId, SyncConflict};
use dirt_core::services::DatabaseService as CoreDatabaseService;
use dirt_core::{Error, Result};

#[cfg(target_os = "android")]
use crate::config::resolve_sync_config;

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
        let resolved_sync_config =
            resolve_sync_config().map_err(|error| Error::InvalidInput(error.to_string()))?;

        let db = CoreDatabaseService::open_path(db_path, resolved_sync_config.sync_config).await?;
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

    /// Create attachment metadata for a note.
    pub async fn create_attachment(
        &self,
        note_id: &NoteId,
        filename: &str,
        mime_type: &str,
        size_bytes: i64,
        r2_key: &str,
    ) -> Result<Attachment> {
        self.db
            .create_attachment(note_id, filename, mime_type, size_bytes, r2_key)
            .await
    }

    /// List attachment metadata for a note.
    pub async fn list_attachments(&self, note_id: &NoteId) -> Result<Vec<Attachment>> {
        self.db.list_attachments(note_id).await
    }

    /// Soft delete attachment metadata by id.
    pub async fn delete_attachment(&self, attachment_id: &AttachmentId) -> Result<()> {
        self.db.delete_attachment(attachment_id).await
    }

    /// List recently resolved sync conflicts.
    pub async fn list_conflicts(&self, limit: usize) -> Result<Vec<SyncConflict>> {
        self.db.list_conflicts(limit).await
    }

    /// Sync with remote database (if configured).
    pub async fn sync(&self) -> Result<()> {
        self.db.sync().await
    }

    /// Check whether remote sync is enabled.
    pub async fn is_sync_enabled(&self) -> bool {
        self.db.is_sync_enabled().await
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
        .unwrap_or_else(|| panic!("Failed to resolve mobile data directory for database"))
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

    #[tokio::test(flavor = "multi_thread")]
    async fn list_all_notes_returns_full_collection() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();
        store.create_note("One").await.unwrap();
        store.create_note("Two").await.unwrap();
        store.create_note("Three").await.unwrap();

        let notes = store.list_all_notes().await.unwrap();
        assert_eq!(notes.len(), 3);
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
    async fn in_memory_store_conflict_list_defaults_empty() {
        let store = MobileNoteStore::open_in_memory().await.unwrap();
        let conflicts = store.list_conflicts(10).await.unwrap();
        assert!(conflicts.is_empty());
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

        store.delete_attachment(&created.id).await.unwrap();
        let attachments = store.list_attachments(&note.id).await.unwrap();
        assert!(attachments.is_empty());
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
}
