//! Note repository implementation

#![allow(clippy::cast_possible_wrap)] // SQLite uses i64 for LIMIT/OFFSET

use crate::error::{Error, Result};
use crate::models::{Note, NoteId, Tag, TagId, extract_tags};
use libsql::Connection;

/// Trait for note storage operations (async)
#[allow(async_fn_in_trait)]
pub trait NoteRepository {
    /// Create a new note
    async fn create(&self, content: &str) -> Result<Note>;

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
    async fn create(&self, content: &str) -> Result<Note> {
        let note = Note::new(content);
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

        Ok(note.clone())
    }

    async fn get(&self, id: &NoteId) -> Result<Option<Note>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS} FROM notes WHERE id = ? AND deleted_at IS NULL"
        );
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

        self.get(id)
            .await?
            .ok_or_else(|| Error::NotFound(id.to_string()))
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
            .execute(
                "DELETE FROM note_tags WHERE note_id = ?",
                [id.as_str()],
            )
            .await?;

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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SOLO_USER_ID;
    use crate::db::Database;

    async fn setup() -> Database {
        Database::open_in_memory().await.unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_create_and_get() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create("Hello world #test").await.unwrap();
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

        repo.create("Note 1").await.unwrap();
        repo.create("Note 2").await.unwrap();
        repo.create("Note 3").await.unwrap();

        let notes = repo.list(10, 0).await.unwrap();
        assert_eq!(notes.len(), 3);
        assert!(notes[0].created_at >= notes[1].created_at);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_update_clears_server_updated_at() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create("Original").await.unwrap();

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

        let note = repo.create("To delete #tagged").await.unwrap();
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

        let alive = repo.create("Hello apple").await.unwrap();
        let doomed = repo.create("Goodbye apple").await.unwrap();
        repo.create("Something else").await.unwrap();

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

        let a = repo.create("#rust one").await.unwrap();
        let _b = repo.create("#rust two").await.unwrap();
        let _c = repo.create("#rust three").await.unwrap();

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

        repo.create("Note with #rust").await.unwrap();
        repo.create("Another #rust note").await.unwrap();
        repo.create("No tag").await.unwrap();

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

        let existing = repo.create("Live #work note").await.unwrap();
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
}
