//! Shared database service wrapper used across clients.
//!
//! Local-only since the Supabase removal: clients sync through the
//! `dirt-api` HTTP backend, not Turso embedded replicas. The recovery
//! plumbing for corrupt local-replica state lived here while embedded
//! replicas were the sync mechanism; with the new design there is no
//! remote replica file to recover from, just a local `SQLite` database.
//!
//! The service carries a `user_id` so every locally-created row gets
//! stamped with whichever account this DB belongs to. Path layout
//! (single DB per user, plus a legacy SOLO path used before first
//! sign-in) lives in [`super::db_paths`].

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::db::{
    Database, LibSqlNoteRepository, LibSqlSettingsRepository, NoteRepository, SettingsRepository,
    SyncCursor,
};
use crate::models::{Note, Settings};
use crate::{NoteId, Result, SOLO_USER_ID};

use super::db_paths::validate_user_id;

/// Thread-safe service for DB and repository operations.
#[derive(Clone)]
pub struct DatabaseService {
    db: Arc<Mutex<Database>>,
    /// Owner of the rows in this DB. New notes are stamped with this
    /// id; sync engines read it via [`Self::user_id`].
    user_id: Arc<str>,
}

impl DatabaseService {
    /// Open the DB at `db_path` and tag it with `user_id`.
    ///
    /// `user_id` must be either [`SOLO_USER_ID`] (legacy / pre-signin)
    /// or a UUID-v7 issued by the server; anything else is rejected
    /// before we touch the filesystem. Use `open_local_path` for the
    /// legacy solo case so the substitution is visible at the call
    /// site.
    pub async fn open_for_user(
        db_path: impl Into<PathBuf>,
        user_id: impl Into<String>,
    ) -> Result<Self> {
        let db_path = db_path.into();
        let user_id = user_id.into();
        validate_user_id(&user_id)?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Database::open(&db_path).await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            user_id: Arc::from(user_id),
        })
    }

    /// Open the legacy single-DB path with the `SOLO_USER_ID` tenant.
    ///
    /// Used on a brand-new machine that has never signed in, and by
    /// existing tests. Production sign-in paths route through
    /// [`Self::open_for_user`] instead.
    pub async fn open_local_path(db_path: impl Into<PathBuf>) -> Result<Self> {
        Self::open_for_user(db_path, SOLO_USER_ID).await
    }

    /// Open an in-memory database service (primarily for tests).
    pub async fn open_in_memory() -> Result<Self> {
        let db = Database::open_in_memory().await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            user_id: Arc::from(SOLO_USER_ID),
        })
    }

    /// In-memory DB tagged with an arbitrary `user_id`. Convenient
    /// for tests that want to exercise per-user scoping without
    /// touching the filesystem.
    pub async fn open_in_memory_for_user(user_id: impl Into<String>) -> Result<Self> {
        let user_id = user_id.into();
        validate_user_id(&user_id)?;
        let db = Database::open_in_memory().await?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            user_id: Arc::from(user_id),
        })
    }

    /// The `user_id` every locally-created row in this DB carries.
    ///
    /// Sync engines pass this into `SyncEngine::new` and the cross-
    /// client mismatch guard (sync workers + CLI sync) compares it
    /// against the bearer's `stored.user_id` before pushing.
    #[must_use]
    pub fn user_id(&self) -> &str {
        &self.user_id
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

    /// Create a new note stamped with the DB's owning `user_id`.
    pub async fn create_note(&self, content: &str) -> Result<Note> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.create(&self.user_id, content).await
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

    // ---- Sync-driver helpers ----
    //
    // The sync engine needs to read/write `pending_sync`, `sync_state`,
    // and stamp server-authoritative timestamps. Each helper takes the
    // mutex briefly so HTTP work can run between calls without holding
    // the DB lock.

    /// Apply a server-authoritative note from a pull response.
    pub async fn upsert_from_server(&self, note: &Note) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.upsert_from_server(note).await
    }

    /// Look up a note's local row, including tombstones.
    pub async fn get_with_tombstone(&self, id: &NoteId) -> Result<Option<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.get_with_tombstone(id).await
    }

    /// Whether `(user_id, note_id)` has an unpushed local mutation.
    pub async fn is_pending(&self, user_id: &str, note_id: &NoteId) -> Result<bool> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.is_pending(user_id, note_id).await
    }

    /// List up to `limit` dirty notes for `user_id`, oldest-first.
    pub async fn list_pending_notes(&self, user_id: &str, limit: usize) -> Result<Vec<Note>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.list_pending_notes(user_id, limit).await
    }

    /// Stamp `server_updated_at` and clear the note's `pending_sync` row.
    pub async fn mark_pushed(
        &self,
        user_id: &str,
        note_id: &NoteId,
        server_updated_at_ms: i64,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.mark_pushed(user_id, note_id, server_updated_at_ms)
            .await
    }

    /// Read the persisted pull cursor for `user_id`.
    pub async fn read_sync_cursor(&self, user_id: &str) -> Result<Option<SyncCursor>> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.read_sync_cursor(user_id).await
    }

    /// Persist the pull cursor for `user_id`.
    pub async fn write_sync_cursor(&self, user_id: &str, cursor: &SyncCursor) -> Result<()> {
        let db = self.db.lock().await;
        let repo = LibSqlNoteRepository::new(db.connection());
        repo.write_sync_cursor(user_id, cursor).await
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
