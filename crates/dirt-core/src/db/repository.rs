//! Note repository implementation

#![allow(clippy::cast_possible_wrap)] // SQLite uses i64 for LIMIT/OFFSET

use crate::error::{Error, Result};
use crate::models::{extract_tags, Note, NoteId, Tag, TagId};
use libsql::Connection;

/// Trait for note storage operations (async)
#[allow(async_fn_in_trait)]
pub trait NoteRepository {
    /// Create a new note stamped with `user_id`.
    ///
    /// Callers pass the active user's id from `DatabaseService::user_id()`
    /// so the new row belongs to whichever account is signed in.
    /// Signed-out (offline) capture passes `SOLO_USER_ID` explicitly.
    async fn create(&self, user_id: &str, content: &str) -> Result<Note>;

    /// Create a note with a pre-generated ID (for optimistic UI updates)
    async fn create_with_note(&self, note: &Note) -> Result<Note>;

    /// Get a note by ID (live rows only)
    async fn get(&self, id: &NoteId) -> Result<Option<Note>>;

    /// List live notes, newest first
    async fn list(&self, limit: usize, offset: usize) -> Result<Vec<Note>>;

    /// Update a note's content
    async fn update(&self, id: &NoteId, content: &str) -> Result<Note>;

    /// Soft-delete a note by stamping `deleted_at`
    async fn delete(&self, id: &NoteId) -> Result<()>;

    /// Search notes by content using FTS
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Note>>;

    /// List notes by tag
    async fn list_by_tag(&self, tag: &str, limit: usize, offset: usize) -> Result<Vec<Note>>;

    /// Get all tags with live-note counts
    async fn list_tags(&self) -> Result<Vec<(String, usize)>>;

    /// Write a server-authoritative note into the local DB.
    ///
    /// Used by the pull-merge path once the pure resolver in
    /// `dirt_core::sync::merge` has decided this row should overwrite the
    /// local copy. Re-populates `note_tags` when the note is live and clears
    /// them on tombstone.
    async fn upsert_from_server(&self, note: &Note) -> Result<()>;

    /// Fetch a note by id, *including* tombstones.
    ///
    /// `get` filters out `deleted_at IS NOT NULL` because UI consumers
    /// want only live rows. The pull-merge path needs the row regardless
    /// of its tombstone state so it can compare `server_updated_at` and
    /// detect un-tombstones.
    async fn get_with_tombstone(&self, id: &NoteId) -> Result<Option<Note>>;

    /// True if `(user_id, note_id)` has an unpushed local mutation.
    async fn is_pending(&self, user_id: &str, note_id: &NoteId) -> Result<bool>;

    /// Record that the named note has unpushed local mutations.
    ///
    /// Idempotent — last-write-wins on `dirty_at`. Called from the
    /// `create_with_note` / `update` / `delete` paths whenever a local
    /// change diverges from the server copy.
    async fn enqueue_pending(
        &self,
        user_id: &str,
        note_id: &NoteId,
        dirty_at_ms: i64,
    ) -> Result<()>;

    /// List up to `limit` notes (live or tombstoned) that have unpushed
    /// mutations, oldest-pending first.
    async fn list_pending_notes(&self, user_id: &str, limit: usize) -> Result<Vec<Note>>;

    /// Stamp `server_updated_at` from the server response and clear the
    /// note's `pending_sync` row. Called after the server accepts a push.
    async fn mark_pushed(
        &self,
        user_id: &str,
        note_id: &NoteId,
        server_updated_at_ms: i64,
    ) -> Result<()>;

    /// Read the pull cursor for the given user, or `None` for "start
    /// from the beginning".
    async fn read_sync_cursor(&self, user_id: &str) -> Result<Option<SyncCursor>>;

    /// Persist the pull cursor for the given user.
    async fn write_sync_cursor(&self, user_id: &str, cursor: &SyncCursor) -> Result<()>;
}

/// Pull cursor stored in `sync_state`.
///
/// `sua` is the last-seen `server_updated_at` from a pull response; `id`
/// is the tie-break for rows sharing that timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncCursor {
    pub sua: i64,
    pub id: String,
}

/// libSQL implementation of `NoteRepository`
pub struct LibSqlNoteRepository<'a> {
    conn: &'a Connection,
}

impl<'a> LibSqlNoteRepository<'a> {
    /// Create a new repository with the given connection
    pub const fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Sync tags for a note (create new tags, link/unlink as needed)
    async fn sync_tags(&self, note_id: &NoteId, content: &str) -> Result<()> {
        let tags = extract_tags(content);

        // Remove all existing tag links for this note
        self.conn
            .execute(
                "DELETE FROM note_tags WHERE note_id = ?",
                [note_id.as_str()],
            )
            .await?;

        // Add new tag links
        for tag_name in tags {
            let tag_id = self.get_or_create_tag(&tag_name).await?;

            self.conn
                .execute(
                    "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?, ?)",
                    [note_id.as_str(), tag_id.as_str()],
                )
                .await?;
        }

        Ok(())
    }

    /// Get or create a tag by name
    async fn get_or_create_tag(&self, name: &str) -> Result<TagId> {
        let mut rows = self
            .conn
            .query("SELECT id FROM tags WHERE name = ? COLLATE NOCASE", [name])
            .await?;

        if let Some(row) = rows.next().await? {
            let id: String = row.get(0)?;
            return id
                .parse()
                .map_err(|_| Error::InvalidInput("Invalid tag ID".into()));
        }

        let tag = Tag::new(name);
        self.conn
            .execute(
                "INSERT INTO tags (id, name, created_at) VALUES (?, ?, ?)",
                libsql::params![tag.id.as_str(), tag.name.as_str(), tag.created_at],
            )
            .await?;

        Ok(tag.id)
    }

    /// Parse a note from a database row.
    ///
    /// Expected column order:
    /// `id, user_id, content, created_at, updated_at, server_updated_at, deleted_at`
    fn parse_note(row: &libsql::Row) -> Result<Note> {
        let id: String = row.get(0)?;
        Ok(Note {
            id: id
                .parse()
                .map_err(|_| Error::InvalidInput(format!("Invalid note ID in database: {id}")))?,
            user_id: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            server_updated_at: row.get(5)?,
            deleted_at: row.get(6)?,
        })
    }
}

const NOTE_COLUMNS: &str =
    "id, user_id, content, created_at, updated_at, server_updated_at, deleted_at";

impl NoteRepository for LibSqlNoteRepository<'_> {
    async fn create(&self, user_id: &str, content: &str) -> Result<Note> {
        let note = Note::new_for_user(content, user_id)?;
        self.create_with_note(&note).await
    }

    async fn create_with_note(&self, note: &Note) -> Result<Note> {
        self.conn
            .execute(
                "INSERT INTO notes (id, user_id, content, created_at, updated_at, server_updated_at, deleted_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                libsql::params![
                    note.id.as_str(),
                    note.user_id.as_str(),
                    note.content.as_str(),
                    note.created_at,
                    note.updated_at,
                    note.server_updated_at,
                    note.deleted_at,
                ],
            )
            .await?;

        if note.deleted_at.is_none() {
            self.sync_tags(&note.id, &note.content).await?;
        }

        // A locally-created note is never server-acknowledged yet, so it
        // must enter the push queue. Skip when the caller is replaying a
        // server-stamped note (server_updated_at present) — that's the
        // pull-merge path going through `upsert_from_server` instead.
        if note.server_updated_at.is_none() {
            self.enqueue_pending(&note.user_id, &note.id, note.updated_at)
                .await?;
        }

        Ok(note.clone())
    }

    async fn get(&self, id: &NoteId) -> Result<Option<Note>> {
        let sql = format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ? AND deleted_at IS NULL");
        let mut rows = self.conn.query(&sql, [id.as_str()]).await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Self::parse_note(&row)?))
        } else {
            Ok(None)
        }
    }

    async fn list(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes
             WHERE deleted_at IS NULL
             ORDER BY updated_at DESC
             LIMIT ? OFFSET ?"
        );
        let mut rows = self
            .conn
            .query(&sql, libsql::params![limit as i64, offset as i64])
            .await?;

        let mut notes = Vec::new();
        while let Some(row) = rows.next().await? {
            notes.push(Self::parse_note(&row)?);
        }

        Ok(notes)
    }

    async fn update(&self, id: &NoteId, content: &str) -> Result<Note> {
        let now = chrono::Utc::now().timestamp_millis();

        let rows_affected = self
            .conn
            .execute(
                "UPDATE notes
                 SET content = ?, updated_at = ?, server_updated_at = NULL
                 WHERE id = ? AND deleted_at IS NULL",
                libsql::params![content, now, id.as_str()],
            )
            .await?;

        if rows_affected == 0 {
            return Err(Error::NotFound(id.to_string()));
        }

        self.sync_tags(id, content).await?;

        let note = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        self.enqueue_pending(&note.user_id, id, now).await?;
        Ok(note)
    }

    async fn delete(&self, id: &NoteId) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();

        let rows_affected = self
            .conn
            .execute(
                "UPDATE notes
                 SET deleted_at = ?, updated_at = ?, server_updated_at = NULL
                 WHERE id = ? AND deleted_at IS NULL",
                libsql::params![now, now, id.as_str()],
            )
            .await?;

        if rows_affected == 0 {
            return Err(Error::NotFound(id.to_string()));
        }

        self.conn
            .execute("DELETE FROM note_tags WHERE note_id = ?", [id.as_str()])
            .await?;

        // The tombstone needs to reach the server too. Look up the
        // user_id on the now-tombstoned row so we can scope the
        // pending entry correctly.
        if let Some(tombstoned) = self.get_with_tombstone(id).await? {
            self.enqueue_pending(&tombstoned.user_id, id, now).await?;
        }

        Ok(())
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        if query.trim().is_empty() {
            return self.list(limit, 0).await;
        }

        let sql = format!(
            "SELECT {}
             FROM notes n
             JOIN notes_fts fts ON n.rowid = fts.rowid
             WHERE notes_fts MATCH ? AND n.deleted_at IS NULL
             ORDER BY rank
             LIMIT ?",
            NOTE_COLUMNS
                .split(", ")
                .map(|c| format!("n.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let mut rows = self
            .conn
            .query(&sql, libsql::params![query, limit as i64])
            .await?;

        let mut notes = Vec::new();
        while let Some(row) = rows.next().await? {
            notes.push(Self::parse_note(&row)?);
        }

        Ok(notes)
    }

    async fn list_by_tag(&self, tag: &str, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let sql = format!(
            "SELECT {}
             FROM notes n
             JOIN note_tags nt ON n.id = nt.note_id
             JOIN tags t ON nt.tag_id = t.id
             WHERE t.name = ? COLLATE NOCASE AND n.deleted_at IS NULL
             ORDER BY n.updated_at DESC
             LIMIT ? OFFSET ?",
            NOTE_COLUMNS
                .split(", ")
                .map(|c| format!("n.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let mut rows = self
            .conn
            .query(&sql, libsql::params![tag, limit as i64, offset as i64])
            .await?;

        let mut notes = Vec::new();
        while let Some(row) = rows.next().await? {
            notes.push(Self::parse_note(&row)?);
        }

        Ok(notes)
    }

    async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        // Prior bug: counted `nt.note_id` which stayed non-NULL even when the
        // joined note was tombstoned, inflating counts. Counting `n.id` with a
        // deleted_at filter in the JOIN produces NULL for tombstoned rows, so
        // only live notes are tallied.
        let mut rows = self
            .conn
            .query(
                "SELECT t.name, COUNT(n.id) as count
                 FROM tags t
                 LEFT JOIN note_tags nt ON t.id = nt.tag_id
                 LEFT JOIN notes n ON nt.note_id = n.id AND n.deleted_at IS NULL
                 GROUP BY t.id
                 HAVING count > 0
                 ORDER BY count DESC, t.name ASC",
                (),
            )
            .await?;

        let mut tags = Vec::new();
        while let Some(row) = rows.next().await? {
            let name: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            tags.push((name, count as usize));
        }

        Ok(tags)
    }

    async fn upsert_from_server(&self, note: &Note) -> Result<()> {
        // The note row and its tag links must land together or not at
        // all — a crash between them would leave FTS / tag-listing views
        // pointing at the previous content's tags. The next pull would
        // re-deliver the same row and reconcile, but a transaction
        // closes the inconsistency window outright at the cost of a
        // single extra round-trip.
        let result: Result<()> = async {
            self.conn.execute("BEGIN", ()).await?;

            self.conn
                .execute(
                    "INSERT INTO notes (id, user_id, content, created_at, updated_at, server_updated_at, deleted_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(id) DO UPDATE SET
                         user_id = excluded.user_id,
                         content = excluded.content,
                         created_at = excluded.created_at,
                         updated_at = excluded.updated_at,
                         server_updated_at = excluded.server_updated_at,
                         deleted_at = excluded.deleted_at",
                    libsql::params![
                        note.id.as_str(),
                        note.user_id.as_str(),
                        note.content.as_str(),
                        note.created_at,
                        note.updated_at,
                        note.server_updated_at,
                        note.deleted_at,
                    ],
                )
                .await?;

            if note.deleted_at.is_some() {
                self.conn
                    .execute(
                        "DELETE FROM note_tags WHERE note_id = ?",
                        [note.id.as_str()],
                    )
                    .await?;
            } else {
                self.sync_tags(&note.id, &note.content).await?;
            }

            self.conn.execute("COMMIT", ()).await?;
            Ok(())
        }
        .await;

        if result.is_err() {
            self.conn.execute("ROLLBACK", ()).await.ok();
        }
        result
    }

    async fn get_with_tombstone(&self, id: &NoteId) -> Result<Option<Note>> {
        let sql = format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?");
        let mut rows = self.conn.query(&sql, [id.as_str()]).await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Self::parse_note(&row)?))
        } else {
            Ok(None)
        }
    }

    async fn is_pending(&self, user_id: &str, note_id: &NoteId) -> Result<bool> {
        let mut rows = self
            .conn
            .query(
                "SELECT 1 FROM pending_sync WHERE user_id = ? AND note_id = ? LIMIT 1",
                libsql::params![user_id, note_id.as_str()],
            )
            .await?;
        Ok(rows.next().await?.is_some())
    }

    async fn enqueue_pending(
        &self,
        user_id: &str,
        note_id: &NoteId,
        dirty_at_ms: i64,
    ) -> Result<()> {
        // Latest mutation wins on dirty_at. Push order is by dirty_at,
        // so refreshing it lets repeated edits coalesce into a single
        // push of the final state without losing FIFO across distinct
        // notes.
        self.conn
            .execute(
                "INSERT INTO pending_sync (user_id, note_id, dirty_at)
                 VALUES (?, ?, ?)
                 ON CONFLICT(user_id, note_id) DO UPDATE SET dirty_at = excluded.dirty_at",
                libsql::params![user_id, note_id.as_str(), dirty_at_ms],
            )
            .await?;
        Ok(())
    }

    async fn list_pending_notes(&self, user_id: &str, limit: usize) -> Result<Vec<Note>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT {}
             FROM notes n
             JOIN pending_sync p ON p.note_id = n.id AND p.user_id = n.user_id
             WHERE p.user_id = ?
             ORDER BY p.dirty_at ASC, n.id ASC
             LIMIT ?",
            NOTE_COLUMNS
                .split(", ")
                .map(|c| format!("n.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let mut rows = self
            .conn
            .query(&sql, libsql::params![user_id, limit])
            .await?;

        let mut notes = Vec::new();
        while let Some(row) = rows.next().await? {
            notes.push(Self::parse_note(&row)?);
        }
        Ok(notes)
    }

    async fn mark_pushed(
        &self,
        user_id: &str,
        note_id: &NoteId,
        server_updated_at_ms: i64,
    ) -> Result<()> {
        // Stamp the server timestamp first so the pending row's removal is
        // the *last* effect — on crash between the two, `is_pending` will
        // still return true and the next sync will retry the push, which
        // the server treats idempotently.
        self.conn
            .execute(
                "UPDATE notes SET server_updated_at = ? WHERE id = ? AND user_id = ?",
                libsql::params![server_updated_at_ms, note_id.as_str(), user_id],
            )
            .await?;
        self.conn
            .execute(
                "DELETE FROM pending_sync WHERE user_id = ? AND note_id = ?",
                libsql::params![user_id, note_id.as_str()],
            )
            .await?;
        Ok(())
    }

    async fn read_sync_cursor(&self, user_id: &str) -> Result<Option<SyncCursor>> {
        let mut rows = self
            .conn
            .query(
                "SELECT cursor_sua, cursor_id FROM sync_state WHERE user_id = ?",
                [user_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        let sua: i64 = row.get(0)?;
        let id: Option<String> = row.get(1).ok();
        // Treat (0, NULL) as "no cursor yet" so the first pull starts at
        // the beginning. Anything else is a real cursor.
        match id {
            Some(id) if sua > 0 => Ok(Some(SyncCursor { sua, id })),
            _ => Ok(None),
        }
    }

    async fn write_sync_cursor(&self, user_id: &str, cursor: &SyncCursor) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO sync_state (user_id, cursor_sua, cursor_id)
                 VALUES (?, ?, ?)
                 ON CONFLICT(user_id) DO UPDATE SET
                     cursor_sua = excluded.cursor_sua,
                     cursor_id  = excluded.cursor_id",
                libsql::params![user_id, cursor.sua, cursor.id.as_str()],
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::SOLO_USER_ID;

    async fn setup() -> Database {
        Database::open_in_memory().await.unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_create_and_get() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create(SOLO_USER_ID, "Hello world #test").await.unwrap();
        assert_eq!(note.content, "Hello world #test");
        assert_eq!(note.user_id, SOLO_USER_ID);
        assert!(note.server_updated_at.is_none());
        assert!(note.deleted_at.is_none());

        let fetched = repo.get(&note.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, note.id);
        assert_eq!(fetched.content, "Hello world #test");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_list() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        repo.create(SOLO_USER_ID, "Note 1").await.unwrap();
        repo.create(SOLO_USER_ID, "Note 2").await.unwrap();
        repo.create(SOLO_USER_ID, "Note 3").await.unwrap();

        let notes = repo.list(10, 0).await.unwrap();
        assert_eq!(notes.len(), 3);
        assert!(notes[0].created_at >= notes[1].created_at);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_update_clears_server_updated_at() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create(SOLO_USER_ID, "Original").await.unwrap();

        // Pretend the note was previously synced.
        db.connection()
            .execute(
                "UPDATE notes SET server_updated_at = 42 WHERE id = ?",
                [note.id.as_str()],
            )
            .await
            .unwrap();

        let updated = repo.update(&note.id, "Updated").await.unwrap();

        assert_eq!(updated.content, "Updated");
        assert!(updated.updated_at >= note.updated_at);
        assert!(
            updated.server_updated_at.is_none(),
            "local mutation must invalidate server_updated_at so the next push re-syncs"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_delete_stamps_deleted_at_and_clears_tags() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create(SOLO_USER_ID, "To delete #tagged").await.unwrap();
        repo.delete(&note.id).await.unwrap();

        // Not visible via live-only queries.
        let fetched = repo.get(&note.id).await.unwrap();
        assert!(fetched.is_none());

        let notes = repo.list(10, 0).await.unwrap();
        assert!(notes.is_empty());

        // Tombstone timestamp actually set.
        let mut rows = db
            .connection()
            .query(
                "SELECT deleted_at FROM notes WHERE id = ?",
                [note.id.as_str()],
            )
            .await
            .unwrap();
        let deleted_at: Option<i64> = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert!(deleted_at.is_some());

        // note_tags row cleared so tag listing no longer surfaces this note.
        let tags = repo.list_tags().await.unwrap();
        assert!(
            tags.iter().all(|(_, count)| *count == 0) || tags.is_empty(),
            "tombstoned note must not contribute to tag counts"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_search_excludes_tombstoned() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let alive = repo.create(SOLO_USER_ID, "Hello apple").await.unwrap();
        let doomed = repo.create(SOLO_USER_ID, "Goodbye apple").await.unwrap();
        repo.create(SOLO_USER_ID, "Something else").await.unwrap();

        let results = repo.search("apple", 10).await.unwrap();
        assert_eq!(results.len(), 2);

        repo.delete(&doomed.id).await.unwrap();
        let results = repo.search("apple", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, alive.id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_tags_counts_skip_tombstoned_notes() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let a = repo.create(SOLO_USER_ID, "#rust one").await.unwrap();
        let _b = repo.create(SOLO_USER_ID, "#rust two").await.unwrap();
        let _c = repo.create(SOLO_USER_ID, "#rust three").await.unwrap();

        let tags = repo.list_tags().await.unwrap();
        let rust_count = tags.iter().find(|(n, _)| n == "rust").unwrap().1;
        assert_eq!(rust_count, 3, "live notes should count");

        repo.delete(&a.id).await.unwrap();
        let tags = repo.list_tags().await.unwrap();
        let rust_count = tags.iter().find(|(n, _)| n == "rust").unwrap().1;
        assert_eq!(
            rust_count, 2,
            "tombstoned note must not be counted (prior bug counted it)"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_list_by_tag() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        repo.create(SOLO_USER_ID, "Note with #rust").await.unwrap();
        repo.create(SOLO_USER_ID, "Another #rust note").await.unwrap();
        repo.create(SOLO_USER_ID, "No tag").await.unwrap();

        let notes = repo.list_by_tag("rust", 10, 0).await.unwrap();
        assert_eq!(notes.len(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_upsert_from_server_inserts_new_live_note() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = Note {
            id: NoteId::new(),
            user_id: SOLO_USER_ID.to_string(),
            content: "From the server #sync".to_string(),
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_000_500,
            server_updated_at: Some(1_700_000_001_000),
            deleted_at: None,
        };
        repo.upsert_from_server(&note).await.unwrap();

        let fetched = repo.get(&note.id).await.unwrap().unwrap();
        assert_eq!(fetched.content, note.content);
        assert_eq!(fetched.server_updated_at, Some(1_700_000_001_000));

        let by_tag = repo.list_by_tag("sync", 10, 0).await.unwrap();
        assert_eq!(by_tag.len(), 1, "sync_tags must run for live upserts");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_upsert_from_server_tombstones_and_clears_tags() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let existing = repo.create(SOLO_USER_ID, "Live #work note").await.unwrap();
        assert_eq!(repo.list_by_tag("work", 10, 0).await.unwrap().len(), 1);

        let tombstoned = Note {
            id: existing.id,
            user_id: existing.user_id.clone(),
            content: existing.content.clone(),
            created_at: existing.created_at,
            updated_at: existing.updated_at + 1,
            server_updated_at: Some(existing.updated_at + 10),
            deleted_at: Some(existing.updated_at + 5),
        };
        repo.upsert_from_server(&tombstoned).await.unwrap();

        // Not visible via live queries.
        assert!(repo.get(&existing.id).await.unwrap().is_none());
        // Tag listing no longer surfaces it.
        assert_eq!(repo.list_by_tag("work", 10, 0).await.unwrap().len(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_enqueues_pending_sync() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create(SOLO_USER_ID, "hello sync").await.unwrap();
        assert!(repo.is_pending(SOLO_USER_ID, &note.id).await.unwrap());

        let pending = repo.list_pending_notes(SOLO_USER_ID, 10).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, note.id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn upsert_from_server_does_not_enqueue_pending() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = Note {
            id: NoteId::new(),
            user_id: SOLO_USER_ID.to_string(),
            content: "from server".to_string(),
            created_at: 1,
            updated_at: 2,
            server_updated_at: Some(3),
            deleted_at: None,
        };
        repo.upsert_from_server(&note).await.unwrap();

        assert!(!repo.is_pending(SOLO_USER_ID, &note.id).await.unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn update_refreshes_pending_dirty_at() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create(SOLO_USER_ID, "first").await.unwrap();
        // Pretend we pushed it so the pending flag is gone, mimicking the
        // post-push state.
        repo.mark_pushed(SOLO_USER_ID, &note.id, 1_000)
            .await
            .unwrap();
        assert!(!repo.is_pending(SOLO_USER_ID, &note.id).await.unwrap());

        repo.update(&note.id, "second").await.unwrap();
        assert!(repo.is_pending(SOLO_USER_ID, &note.id).await.unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delete_enqueues_tombstone_for_push() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create(SOLO_USER_ID, "doomed").await.unwrap();
        repo.mark_pushed(SOLO_USER_ID, &note.id, 100).await.unwrap();
        repo.delete(&note.id).await.unwrap();

        assert!(repo.is_pending(SOLO_USER_ID, &note.id).await.unwrap());

        let pending = repo.list_pending_notes(SOLO_USER_ID, 10).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].is_deleted());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mark_pushed_stamps_sua_and_clears_pending() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create(SOLO_USER_ID, "hello").await.unwrap();
        assert!(repo.is_pending(SOLO_USER_ID, &note.id).await.unwrap());

        repo.mark_pushed(SOLO_USER_ID, &note.id, 12_345)
            .await
            .unwrap();

        assert!(!repo.is_pending(SOLO_USER_ID, &note.id).await.unwrap());
        let stored = repo.get(&note.id).await.unwrap().unwrap();
        assert_eq!(stored.server_updated_at, Some(12_345));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_pending_notes_orders_by_dirty_at() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let a = repo.create(SOLO_USER_ID, "a").await.unwrap();
        let b = repo.create(SOLO_USER_ID, "b").await.unwrap();
        let c = repo.create(SOLO_USER_ID, "c").await.unwrap();

        // Re-stamp dirty_at on `b` to a far-future value so it becomes the
        // newest dirty row regardless of how close create()'s wall-clock
        // timestamps end up to each other.
        repo.enqueue_pending(SOLO_USER_ID, &b.id, i64::MAX / 2)
            .await
            .unwrap();

        let pending = repo.list_pending_notes(SOLO_USER_ID, 10).await.unwrap();
        let ids: Vec<NoteId> = pending.iter().map(|n| n.id).collect();
        // a and c keep their original (smaller) dirty_at, b is newest.
        assert_eq!(ids.last().copied(), Some(b.id));
        assert!(ids.contains(&a.id));
        assert!(ids.contains(&c.id));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn get_with_tombstone_returns_deleted_rows() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create(SOLO_USER_ID, "doomed").await.unwrap();
        repo.delete(&note.id).await.unwrap();

        assert!(repo.get(&note.id).await.unwrap().is_none());
        let tombstone = repo
            .get_with_tombstone(&note.id)
            .await
            .unwrap()
            .expect("tombstoned row should still be visible to sync");
        assert!(tombstone.is_deleted());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sync_cursor_roundtrips() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        assert!(
            repo.read_sync_cursor(SOLO_USER_ID).await.unwrap().is_none(),
            "fresh DB has no cursor"
        );

        let cursor = SyncCursor {
            sua: 1_700_000_000_000,
            id: "01932aaa-0000-7000-8000-000000000abc".to_string(),
        };
        repo.write_sync_cursor(SOLO_USER_ID, &cursor).await.unwrap();

        let restored = repo.read_sync_cursor(SOLO_USER_ID).await.unwrap().unwrap();
        assert_eq!(restored, cursor);
    }
}
