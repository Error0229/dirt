//! Database service for the desktop application

#![allow(dead_code)] // Methods will be used in future features

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dirt_core::db::{
    Database, NoteRepository, SettingsRepository, SqliteNoteRepository, SqliteSettingsRepository,
};
use dirt_core::error::Result;
use dirt_core::models::{Note, Settings};
use dirt_core::NoteId;

/// Service for database operations
///
/// Wraps the database connection and provides thread-safe access.
#[derive(Clone)]
pub struct DatabaseService {
    db: Arc<Mutex<Database>>,
}

impl DatabaseService {
    /// Create a new database service with the default data directory
    pub fn new() -> Result<Self> {
        let db_path = Self::default_db_path();

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Database::open(&db_path)?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }

    /// Create an in-memory database service (for testing)
    #[allow(dead_code)]
    pub fn in_memory() -> Result<Self> {
        let db = Database::open_in_memory()?;
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

    /// List all notes
    pub fn list_notes(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteNoteRepository::new(db.connection());
        repo.list(limit, offset)
    }

    /// Get a note by ID
    pub fn get_note(&self, id: &NoteId) -> Result<Option<Note>> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteNoteRepository::new(db.connection());
        repo.get(id)
    }

    /// Create a new note
    pub fn create_note(&self, content: &str) -> Result<Note> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteNoteRepository::new(db.connection());
        repo.create(content)
    }

    /// Update a note
    pub fn update_note(&self, id: &NoteId, content: &str) -> Result<Note> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteNoteRepository::new(db.connection());
        repo.update(id, content)
    }

    /// Delete a note (soft delete)
    pub fn delete_note(&self, id: &NoteId) -> Result<()> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteNoteRepository::new(db.connection());
        repo.delete(id)
    }

    /// Search notes
    pub fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteNoteRepository::new(db.connection());
        repo.search(query, limit)
    }

    /// List notes by tag
    pub fn list_notes_by_tag(&self, tag: &str, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteNoteRepository::new(db.connection());
        repo.list_by_tag(tag, limit, offset)
    }

    /// Get all tags with counts
    pub fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteNoteRepository::new(db.connection());
        repo.list_tags()
    }

    /// Load settings from database
    pub fn load_settings(&self) -> Result<Settings> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteSettingsRepository::new(db.connection());
        repo.load()
    }

    /// Save settings to database
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let db = self.db.lock().map_err(|_| {
            dirt_core::error::Error::Database("Failed to acquire database lock".into())
        })?;
        let repo = SqliteSettingsRepository::new(db.connection());
        repo.save(settings)
    }
}
