//! Shared database service wrapper used across clients.
//!
//! Local-only since the Supabase removal: clients sync through the
//! `dirt-api` HTTP backend, not Turso embedded replicas. The recovery
//! plumbing for corrupt local-replica state lived here while embedded
//! replicas were the sync mechanism; with the new design there is no
//! remote replica file to recover from, just a local `SQLite` database.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::db::{
    Database, LibSqlNoteRepository, LibSqlSettingsRepository, NoteRepository, SettingsRepository,
};
use crate::models::{Note, Settings};
use crate::{NoteId, Result};

/// Thread-safe service for DB and repository operations.
#[derive(Clone)]
pub struct DatabaseService {
    db: Arc<Mutex<Database>>,
}

impl DatabaseService {
    /// Open a local-only database service at the given path.
    pub async fn open_local_path(db_path: impl Into<PathBuf>) -> Result<Self> {
        let db_path = db_path.into();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Database::open(&db_path).await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }

    /// Open an in-memory database service (primarily for tests).
    pub async fn open_in_memory() -> Result<Self> {
        let db = Database::open_in_memory().await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
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

    /// Find recent non-deleted note IDs by id prefix.
    pub async fn list_note_ids_by_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<String>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let db = self.db.lock().await;
        let mut rows = db
            .connection()
            .query(
                "SELECT id
                 FROM notes
                 WHERE deleted_at IS NULL AND id LIKE ?
                 ORDER BY updated_at DESC
                 LIMIT ?",
                libsql::params![format!("{prefix}%"), limit],
            )
            .await?;

        let mut matching_ids = Vec::new();
        while let Some(row) = rows.next().await? {
            let id: String = row.get(0)?;
            matching_ids.push(id);
        }

        Ok(matching_ids)
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
}
