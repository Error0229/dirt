//! Shared async database wrapper for app clients.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::db::{
    Database, LibSqlNoteRepository, LibSqlSettingsRepository, NoteRepository, SettingsRepository,
};
use crate::models::{Attachment, AttachmentId, Note, Settings};
use crate::{NoteId, Result, SyncConflict};

#[derive(Clone)]
pub struct DatabaseService {
    db: Arc<Mutex<Database>>,
}

impl DatabaseService {
    #[must_use]
    pub fn from_database(db: Database) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
        }
    }

    pub async fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::open(db_path.as_ref()).await?;
        Ok(Self::from_database(db))
    }

    pub async fn open_with_sync(
        db_path: impl AsRef<Path>,
        sync_config: crate::db::SyncConfig,
    ) -> Result<Self> {
        let db = Database::open_with_sync(db_path.as_ref(), sync_config).await?;
        Ok(Self::from_database(db))
    }

    pub async fn open_in_memory() -> Result<Self> {
        let db = Database::open_in_memory().await?;
        Ok(Self::from_database(db))
    }

    pub async fn sync(&self) -> Result<()> {
        let db = self.db.lock().await;
        db.sync().await
    }

    pub async fn is_sync_enabled(&self) -> bool {
        let db = self.db.lock().await;
        db.is_sync_enabled()
    }

    pub async fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list(limit, offset).await
    }

    pub async fn list_all_notes(&self) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list(usize::MAX, 0).await
    }

    pub async fn get_note(&self, id: &NoteId) -> Result<Option<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.get(id).await
    }

    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create(content).await
    }

    pub async fn create_note_with_id(&self, note: &Note) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create_with_note(note).await
    }

    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.update(id, content).await
    }

    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.delete(id).await
    }

    pub async fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.search(query, limit).await
    }

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

    pub async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_tags().await
    }

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

    pub async fn list_attachments(&self, note_id: &NoteId) -> Result<Vec<Attachment>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_attachments(note_id).await
    }

    pub async fn delete_attachment(&self, attachment_id: &AttachmentId) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.delete_attachment(attachment_id).await
    }

    pub async fn list_conflicts(&self, limit: usize) -> Result<Vec<SyncConflict>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_conflicts(limit).await
    }

    pub async fn load_settings(&self) -> Result<Settings> {
        let db = self.db.lock().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.load().await
    }

    pub async fn save_settings(&self, settings: &Settings) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.save(settings).await
    }
}
