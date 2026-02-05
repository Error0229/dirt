//! Database service for the desktop application

#![allow(dead_code)] // Methods will be used in future features

use std::path::PathBuf;
use std::sync::Arc;

use dirt_core::db::{
    Database, LibSqlNoteRepository, LibSqlSettingsRepository, NoteRepository, SettingsRepository,
    SyncConfig,
};
use dirt_core::error::Result;
use dirt_core::models::{Note, Settings};
use dirt_core::NoteId;
use tokio::sync::Mutex;

/// Service for database operations
///
/// Wraps the database connection and provides thread-safe async access.
#[derive(Clone)]
pub struct DatabaseService {
    db: Arc<Mutex<Database>>,
}

impl DatabaseService {
    /// Create a new database service, auto-detecting sync config from environment
    ///
    /// If TURSO_DATABASE_URL and TURSO_AUTH_TOKEN are set, sync is enabled automatically.
    /// Otherwise, falls back to local-only mode.
    pub async fn new() -> Result<Self> {
        let db_path = Self::default_db_path();

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Check for sync config from environment
        let sync_config = Self::sync_config_from_env();

        // Run database initialization in a thread with larger stack (8MB)
        // to handle deep libsql call stacks during sync
        let db = if let Some(config) = sync_config {
            tracing::info!(
                "Sync enabled with Turso: {}",
                config.url.as_deref().unwrap_or("unknown")
            );
            let path = db_path.clone();
            let result = std::thread::Builder::new()
                .stack_size(8 * 1024 * 1024) // 8MB stack
                .spawn(move || {
                    tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .unwrap()
                        .block_on(Database::open_with_sync(&path, config))
                })
                .map_err(|e| dirt_core::error::Error::Database(e.to_string()))?
                .join()
                .map_err(|_| dirt_core::error::Error::Database("Thread panicked".to_string()))??;
            result
        } else {
            tracing::info!("Running in local-only mode (no TURSO_DATABASE_URL/TURSO_AUTH_TOKEN)");
            Database::open(&db_path).await?
        };

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }

    /// Read sync configuration from environment variables
    fn sync_config_from_env() -> Option<SyncConfig> {
        let url = std::env::var("TURSO_DATABASE_URL").ok()?;
        let token = std::env::var("TURSO_AUTH_TOKEN").ok()?;

        if url.is_empty() || token.is_empty() {
            return None;
        }

        Some(SyncConfig::new(url, token))
    }

    /// Create a new database service with explicit sync config
    pub async fn new_with_sync(sync_config: SyncConfig) -> Result<Self> {
        let db_path = Self::default_db_path();

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Database::open_with_sync(&db_path, sync_config).await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }

    /// Create an in-memory database service (for testing)
    #[allow(dead_code)]
    pub async fn in_memory() -> Result<Self> {
        let db = Database::open_in_memory().await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }

    /// Get the default database path
    fn default_db_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dirt")
            .join("dirt.db")
    }

    /// Sync with remote database (if configured)
    pub async fn sync(&self) -> Result<()> {
        let db = self.db.lock().await;
        db.sync().await
    }

    /// Check if sync is enabled
    pub async fn is_sync_enabled(&self) -> bool {
        let db = self.db.lock().await;
        db.is_sync_enabled()
    }

    /// List all notes
    pub async fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list(limit, offset).await
    }

    /// Get a note by ID
    pub async fn get_note(&self, id: &NoteId) -> Result<Option<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.get(id).await
    }

    /// Create a new note
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create(content).await
    }

    /// Update a note
    pub async fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.update(id, content).await
    }

    /// Delete a note (soft delete)
    pub async fn delete_note(&self, id: &NoteId) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.delete(id).await
    }

    /// Search notes
    pub async fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.search(query, limit).await
    }

    /// List notes by tag
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

    /// Get all tags with counts
    pub async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_tags().await
    }

    /// Load settings from database
    pub async fn load_settings(&self) -> Result<Settings> {
        let db = self.db.lock().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.load().await
    }

    /// Save settings to database
    pub async fn save_settings(&self, settings: &Settings) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlSettingsRepository::new(db.connection());
        repo.save(settings).await
    }
}
