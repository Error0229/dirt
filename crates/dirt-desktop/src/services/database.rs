//! Desktop database service wrapper.

use std::path::PathBuf;

use dirt_core::db::{Database, SyncConfig};
use dirt_core::models::{Attachment, AttachmentId, Note, Settings};
use dirt_core::services::database::DatabaseService as CoreDatabaseService;
use dirt_core::{NoteId, Result, SyncConflict};

#[derive(Clone)]
pub struct DatabaseService {
    inner: CoreDatabaseService,
}

impl DatabaseService {
    pub async fn new() -> Result<Self> {
        let db_path = Self::default_db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let sync_config = Self::sync_config_from_env();
        let inner = Self::open_database(db_path, sync_config).await?;
        Ok(Self { inner })
    }

    fn sync_config_from_env() -> Option<SyncConfig> {
        let url = std::env::var("TURSO_DATABASE_URL").ok();
        let auth_token = std::env::var("TURSO_AUTH_TOKEN").ok();
        match (url, auth_token) {
            (Some(url), Some(auth_token)) if !url.is_empty() && !auth_token.is_empty() => {
                Some(SyncConfig::new(url, auth_token))
            }
            _ => None,
        }
    }

    pub async fn new_with_sync(sync_config: SyncConfig) -> Result<Self> {
        let db_path = Self::default_db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let inner = Self::open_database(db_path, Some(sync_config)).await?;
        Ok(Self { inner })
    }

    #[allow(dead_code)]
    pub async fn in_memory() -> Result<Self> {
        Ok(Self {
            inner: CoreDatabaseService::open_in_memory().await?,
        })
    }

    async fn open_database(
        db_path: PathBuf,
        sync_config: Option<SyncConfig>,
    ) -> Result<CoreDatabaseService> {
        match sync_config {
            Some(sync_config) => {
                let db = Database::open_with_sync(&db_path, sync_config).await?;
                Ok(CoreDatabaseService::from_database(db))
            }
            None => Ok(CoreDatabaseService::open(&db_path).await?),
        }
    }

    fn default_db_path() -> PathBuf {
        dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dirt")
            .join("dirt.db")
    }

    pub async fn sync(&self) -> Result<()> {
        self.inner.sync().await
    }

    pub async fn is_sync_enabled(&self) -> bool {
        self.inner.is_sync_enabled().await
    }

    pub async fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        self.inner.list_notes(limit, offset).await
    }

    pub async fn get_note(&self, id: &NoteId) -> Result<Option<Note>> {
        self.inner.get_note(id).await
    }

    pub async fn create_note(&self, content: &str) -> Result<Note> {
        self.inner.create_note(content).await
    }

    pub async fn create_note_with_id(&self, note: &Note) -> Result<Note> {
        self.inner.create_note_with_id(note).await
    }

    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        self.inner.update_note(id, content).await
    }

    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        self.inner.delete_note(id).await
    }

    #[allow(dead_code)]
    pub async fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        self.inner.search_notes(query, limit).await
    }

    #[allow(dead_code)]
    pub async fn list_notes_by_tag(
        &self,
        tag: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Note>> {
        self.inner.list_notes_by_tag(tag, limit, offset).await
    }

    #[allow(dead_code)]
    pub async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        self.inner.list_tags().await
    }

    pub async fn list_conflicts(&self, limit: usize) -> Result<Vec<SyncConflict>> {
        self.inner.list_conflicts(limit).await
    }

    pub async fn create_attachment(
        &self,
        note_id: &NoteId,
        filename: &str,
        mime_type: &str,
        size_bytes: i64,
        r2_key: &str,
    ) -> Result<Attachment> {
        self.inner
            .create_attachment(note_id, filename, mime_type, size_bytes, r2_key)
            .await
    }

    pub async fn list_attachments(&self, note_id: &NoteId) -> Result<Vec<Attachment>> {
        self.inner.list_attachments(note_id).await
    }

    pub async fn delete_attachment(&self, attachment_id: &AttachmentId) -> Result<()> {
        self.inner.delete_attachment(attachment_id).await
    }

    pub async fn load_settings(&self) -> Result<Settings> {
        self.inner.load_settings().await
    }

    pub async fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.inner.save_settings(settings).await
    }
}
