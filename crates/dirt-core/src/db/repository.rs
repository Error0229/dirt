//! Note repository implementation

#![allow(clippy::cast_possible_wrap)] // SQLite uses i64 for LIMIT/OFFSET

use crate::error::{Error, Result};
use crate::models::{
    extract_tags, Attachment, AttachmentId, Note, NoteId, SyncConflict, Tag, TagId,
};
use libsql::Connection;

/// Trait for note storage operations (async)
#[allow(async_fn_in_trait)]
pub trait NoteRepository {
    /// Create a new note
    async fn create(&self, content: &str) -> Result<Note>;

    /// Create a note with a pre-generated ID (for optimistic UI updates)
    async fn create_with_note(&self, note: &Note) -> Result<Note>;

    /// Get a note by ID
    async fn get(&self, id: &NoteId) -> Result<Option<Note>>;

    /// List notes (excluding deleted), newest first
    async fn list(&self, limit: usize, offset: usize) -> Result<Vec<Note>>;

    /// Update a note's content
    async fn update(&self, id: &NoteId, content: &str) -> Result<Note>;

    /// Soft delete a note
    async fn delete(&self, id: &NoteId) -> Result<()>;

    /// Search notes by content using FTS
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Note>>;

    /// List notes by tag
    async fn list_by_tag(&self, tag: &str, limit: usize, offset: usize) -> Result<Vec<Note>>;

    /// Get all tags with note counts
    async fn list_tags(&self) -> Result<Vec<(String, usize)>>;

    /// List recently resolved sync conflicts
    async fn list_conflicts(&self, limit: usize) -> Result<Vec<SyncConflict>>;

    /// Create attachment metadata for a note
    async fn create_attachment(
        &self,
        note_id: &NoteId,
        filename: &str,
        mime_type: &str,
        size_bytes: i64,
        r2_key: &str,
    ) -> Result<Attachment>;

    /// List non-deleted attachments for a note
    async fn list_attachments(&self, note_id: &NoteId) -> Result<Vec<Attachment>>;

    /// Soft delete attachment metadata by id
    async fn delete_attachment(&self, attachment_id: &AttachmentId) -> Result<()>;
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
            // Get or create tag
            let tag_id = self.get_or_create_tag(&tag_name).await?;

            // Link tag to note
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
        // Try to find existing tag
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

        // Create new tag
        let tag = Tag::new(name);
        self.conn
            .execute(
                "INSERT INTO tags (id, name, created_at) VALUES (?, ?, ?)",
                libsql::params![tag.id.as_str(), tag.name.as_str(), tag.created_at],
            )
            .await?;

        Ok(tag.id)
    }

    /// Parse a note from a database row
    fn parse_note(row: &libsql::Row) -> Result<Note> {
        let id: String = row.get(0)?;
        Ok(Note {
            id: id.parse().unwrap_or_default(),
            content: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            is_deleted: row.get::<i32>(4)? != 0,
        })
    }

    /// Parse a sync conflict from a database row
    fn parse_conflict(row: &libsql::Row) -> Result<SyncConflict> {
        Ok(SyncConflict {
            id: row.get(0)?,
            note_id: row.get(1)?,
            local_updated_at: row.get(2)?,
            incoming_updated_at: row.get(3)?,
            resolved_at: row.get(4)?,
            strategy: row.get(5)?,
        })
    }

    /// Parse attachment metadata from a database row
    fn parse_attachment(row: &libsql::Row) -> Result<Attachment> {
        let id: String = row.get(0)?;
        let note_id: String = row.get(1)?;
        Ok(Attachment {
            id: id
                .parse()
                .map_err(|_| Error::InvalidInput("Invalid attachment ID".into()))?,
            note_id: note_id
                .parse()
                .map_err(|_| Error::InvalidInput("Invalid note ID".into()))?,
            filename: row.get(2)?,
            mime_type: row.get(3)?,
            size_bytes: row.get(4)?,
            r2_key: row.get(5)?,
            created_at: row.get(6)?,
            is_deleted: row.get::<i32>(7)? != 0,
        })
    }
}

impl NoteRepository for LibSqlNoteRepository<'_> {
    async fn create(&self, content: &str) -> Result<Note> {
        let note = Note::new(content);
        self.create_with_note(&note).await
    }

    async fn create_with_note(&self, note: &Note) -> Result<Note> {
        self.conn
            .execute(
                "INSERT INTO notes (id, content, created_at, updated_at, is_deleted) VALUES (?, ?, ?, ?, ?)",
                libsql::params![
                    note.id.as_str(),
                    note.content.as_str(),
                    note.created_at,
                    note.updated_at,
                    i32::from(note.is_deleted)
                ],
            )
            .await?;

        self.sync_tags(&note.id, &note.content).await?;

        Ok(note.clone())
    }

    async fn get(&self, id: &NoteId) -> Result<Option<Note>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, content, created_at, updated_at, is_deleted FROM notes WHERE id = ? AND is_deleted = 0",
                [id.as_str()],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(Self::parse_note(&row)?))
        } else {
            Ok(None)
        }
    }

    async fn list(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, content, created_at, updated_at, is_deleted
                 FROM notes
                 WHERE is_deleted = 0
                 ORDER BY updated_at DESC
                 LIMIT ? OFFSET ?",
                libsql::params![limit as i64, offset as i64],
            )
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
                "UPDATE notes SET content = ?, updated_at = ? WHERE id = ? AND is_deleted = 0",
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
                "UPDATE notes SET is_deleted = 1, updated_at = ? WHERE id = ? AND is_deleted = 0",
                libsql::params![now, id.as_str()],
            )
            .await?;

        if rows_affected == 0 {
            return Err(Error::NotFound(id.to_string()));
        }

        Ok(())
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        if query.trim().is_empty() {
            return self.list(limit, 0).await;
        }

        let mut rows = self
            .conn
            .query(
                "SELECT n.id, n.content, n.created_at, n.updated_at, n.is_deleted
                 FROM notes n
                 JOIN notes_fts fts ON n.rowid = fts.rowid
                 WHERE notes_fts MATCH ? AND n.is_deleted = 0
                 ORDER BY rank
                 LIMIT ?",
                libsql::params![query, limit as i64],
            )
            .await?;

        let mut notes = Vec::new();
        while let Some(row) = rows.next().await? {
            notes.push(Self::parse_note(&row)?);
        }

        Ok(notes)
    }

    async fn list_by_tag(&self, tag: &str, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let mut rows = self
            .conn
            .query(
                "SELECT n.id, n.content, n.created_at, n.updated_at, n.is_deleted
                 FROM notes n
                 JOIN note_tags nt ON n.id = nt.note_id
                 JOIN tags t ON nt.tag_id = t.id
                 WHERE t.name = ? COLLATE NOCASE AND n.is_deleted = 0
                 ORDER BY n.updated_at DESC
                 LIMIT ? OFFSET ?",
                libsql::params![tag, limit as i64, offset as i64],
            )
            .await?;

        let mut notes = Vec::new();
        while let Some(row) = rows.next().await? {
            notes.push(Self::parse_note(&row)?);
        }

        Ok(notes)
    }

    async fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let mut rows = self
            .conn
            .query(
                "SELECT t.name, COUNT(nt.note_id) as count
                 FROM tags t
                 LEFT JOIN note_tags nt ON t.id = nt.tag_id
                 LEFT JOIN notes n ON nt.note_id = n.id AND n.is_deleted = 0
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

    async fn list_conflicts(&self, limit: usize) -> Result<Vec<SyncConflict>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, note_id, local_updated_at, incoming_updated_at, resolved_at, strategy
                 FROM sync_conflicts
                 ORDER BY resolved_at DESC, id DESC
                 LIMIT ?",
                libsql::params![limit as i64],
            )
            .await?;

        let mut conflicts = Vec::new();
        while let Some(row) = rows.next().await? {
            conflicts.push(Self::parse_conflict(&row)?);
        }

        Ok(conflicts)
    }

    async fn create_attachment(
        &self,
        note_id: &NoteId,
        filename: &str,
        mime_type: &str,
        size_bytes: i64,
        r2_key: &str,
    ) -> Result<Attachment> {
        let mut rows = self
            .conn
            .query(
                "SELECT id FROM notes WHERE id = ? AND is_deleted = 0",
                [note_id.as_str()],
            )
            .await?;
        if rows.next().await?.is_none() {
            return Err(Error::NotFound(note_id.to_string()));
        }

        let attachment = Attachment::new(*note_id, filename, mime_type, size_bytes, r2_key)?;

        self.conn
            .execute(
                "INSERT INTO attachments (
                    id, note_id, filename, mime_type, size_bytes, r2_key, created_at, is_deleted
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                libsql::params![
                    attachment.id.as_str(),
                    attachment.note_id.as_str(),
                    attachment.filename.as_str(),
                    attachment.mime_type.as_str(),
                    attachment.size_bytes,
                    attachment.r2_key.as_str(),
                    attachment.created_at,
                    i32::from(attachment.is_deleted),
                ],
            )
            .await?;

        Ok(attachment)
    }

    async fn list_attachments(&self, note_id: &NoteId) -> Result<Vec<Attachment>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, note_id, filename, mime_type, size_bytes, r2_key, created_at, is_deleted
                 FROM attachments
                 WHERE note_id = ? AND is_deleted = 0
                 ORDER BY created_at DESC, id DESC",
                [note_id.as_str()],
            )
            .await?;

        let mut attachments = Vec::new();
        while let Some(row) = rows.next().await? {
            attachments.push(Self::parse_attachment(&row)?);
        }

        Ok(attachments)
    }

    async fn delete_attachment(&self, attachment_id: &AttachmentId) -> Result<()> {
        let rows_affected = self
            .conn
            .execute(
                "UPDATE attachments
                 SET is_deleted = 1
                 WHERE id = ? AND is_deleted = 0",
                [attachment_id.as_str()],
            )
            .await?;

        if rows_affected == 0 {
            return Err(Error::NotFound(attachment_id.to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        // Should be in reverse chronological order
        assert!(notes[0].created_at >= notes[1].created_at);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_update() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create("Original").await.unwrap();
        let updated = repo.update(&note.id, "Updated").await.unwrap();

        assert_eq!(updated.content, "Updated");
        assert!(updated.updated_at >= note.updated_at);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_delete() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create("To delete").await.unwrap();
        repo.delete(&note.id).await.unwrap();

        // Should not find deleted note
        let fetched = repo.get(&note.id).await.unwrap();
        assert!(fetched.is_none());

        // Should not appear in list
        let notes = repo.list(10, 0).await.unwrap();
        assert!(notes.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_search() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        repo.create("Hello world").await.unwrap();
        repo.create("Goodbye world").await.unwrap();
        repo.create("Something else").await.unwrap();

        let results = repo.search("world", 10).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_tags() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        repo.create("Note with #rust and #programming")
            .await
            .unwrap();
        repo.create("Another #rust note").await.unwrap();
        repo.create("Just #programming").await.unwrap();

        let tags = repo.list_tags().await.unwrap();
        assert_eq!(tags.len(), 2);

        // Rust should have 2 notes
        let rust_tag = tags.iter().find(|(name, _)| name == "rust").unwrap();
        assert_eq!(rust_tag.1, 2);
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
    async fn test_lww_stale_update_is_ignored_and_logged() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create("Current value").await.unwrap();
        let stale_ts = note.updated_at - 10_000;

        repo.conn
            .execute(
                "UPDATE notes SET content = ?, updated_at = ? WHERE id = ?",
                libsql::params!["Stale value", stale_ts, note.id.as_str()],
            )
            .await
            .unwrap();

        let fetched = repo.get(&note.id).await.unwrap().unwrap();
        assert_eq!(fetched.content, "Current value");
        assert_eq!(fetched.updated_at, note.updated_at);

        let conflicts = repo.list_conflicts(10).await.unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].note_id, note.id.to_string());
        assert_eq!(conflicts[0].local_updated_at, note.updated_at);
        assert_eq!(conflicts[0].incoming_updated_at, stale_ts);
        assert_eq!(conflicts[0].strategy, "lww");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_lww_newer_update_wins() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create("Original").await.unwrap();
        let newer_ts = note.updated_at + 10_000;

        repo.conn
            .execute(
                "UPDATE notes SET content = ?, updated_at = ? WHERE id = ?",
                libsql::params!["Newer value", newer_ts, note.id.as_str()],
            )
            .await
            .unwrap();

        let fetched = repo.get(&note.id).await.unwrap().unwrap();
        assert_eq!(fetched.content, "Newer value");
        assert_eq!(fetched.updated_at, newer_ts);

        let conflicts = repo.list_conflicts(10).await.unwrap();
        assert!(conflicts.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_attachment_metadata_crud() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let note = repo.create("Attachment note").await.unwrap();

        let first = repo
            .create_attachment(
                &note.id,
                "image-1.png",
                "image/png",
                1234,
                "notes/a/image-1.png",
            )
            .await
            .unwrap();
        let second = repo
            .create_attachment(
                &note.id,
                "image-2.jpg",
                "image/jpeg",
                4567,
                "notes/a/image-2.jpg",
            )
            .await
            .unwrap();

        let attachments = repo.list_attachments(&note.id).await.unwrap();
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].id, second.id);
        assert_eq!(attachments[1].id, first.id);

        repo.delete_attachment(&second.id).await.unwrap();

        let attachments = repo.list_attachments(&note.id).await.unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].id, first.id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_create_attachment_requires_existing_note() {
        let db = setup().await;
        let repo = LibSqlNoteRepository::new(db.connection());

        let missing_note = NoteId::new();
        let result = repo
            .create_attachment(
                &missing_note,
                "missing.png",
                "image/png",
                1,
                "notes/missing.png",
            )
            .await;
        assert!(matches!(result, Err(Error::NotFound(_))));
    }
}
