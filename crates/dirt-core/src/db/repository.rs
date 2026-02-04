//! Note repository implementation

#![allow(clippy::cast_possible_wrap)] // SQLite uses i64 for LIMIT/OFFSET

use crate::error::{Error, Result};
use crate::models::{extract_tags, Note, NoteId, Tag, TagId};
use rusqlite::{params, Connection};

/// Trait for note storage operations
pub trait NoteRepository {
    /// Create a new note
    fn create(&self, content: &str) -> Result<Note>;

    /// Get a note by ID
    fn get(&self, id: &NoteId) -> Result<Option<Note>>;

    /// List notes (excluding deleted), newest first
    fn list(&self, limit: usize, offset: usize) -> Result<Vec<Note>>;

    /// Update a note's content
    fn update(&self, id: &NoteId, content: &str) -> Result<Note>;

    /// Soft delete a note
    fn delete(&self, id: &NoteId) -> Result<()>;

    /// Search notes by content using FTS
    fn search(&self, query: &str, limit: usize) -> Result<Vec<Note>>;

    /// List notes by tag
    fn list_by_tag(&self, tag: &str, limit: usize, offset: usize) -> Result<Vec<Note>>;

    /// Get all tags with note counts
    fn list_tags(&self) -> Result<Vec<(String, usize)>>;
}

/// `SQLite` implementation of `NoteRepository`
pub struct SqliteNoteRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SqliteNoteRepository<'a> {
    /// Create a new repository with the given connection
    pub const fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Sync tags for a note (create new tags, link/unlink as needed)
    fn sync_tags(&self, note_id: &NoteId, content: &str) -> Result<()> {
        let tags = extract_tags(content);

        // Remove all existing tag links for this note
        self.conn.execute(
            "DELETE FROM note_tags WHERE note_id = ?",
            params![note_id.as_str()],
        )?;

        // Add new tag links
        for tag_name in tags {
            // Get or create tag
            let tag_id = self.get_or_create_tag(&tag_name)?;

            // Link tag to note
            self.conn.execute(
                "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?, ?)",
                params![note_id.as_str(), tag_id.as_str()],
            )?;
        }

        Ok(())
    }

    /// Get or create a tag by name
    fn get_or_create_tag(&self, name: &str) -> Result<TagId> {
        // Try to find existing tag
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM tags WHERE name = ? COLLATE NOCASE",
                params![name],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            return id
                .parse()
                .map_err(|_| Error::InvalidInput("Invalid tag ID".into()));
        }

        // Create new tag
        let tag = Tag::new(name);
        self.conn.execute(
            "INSERT INTO tags (id, name, created_at) VALUES (?, ?, ?)",
            params![tag.id.as_str(), tag.name, tag.created_at],
        )?;

        Ok(tag.id)
    }

    /// Parse a note from a database row
    fn parse_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
        let id: String = row.get(0)?;
        Ok(Note {
            id: id.parse().unwrap_or_default(),
            content: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            is_deleted: row.get::<_, i32>(4)? != 0,
        })
    }
}

impl NoteRepository for SqliteNoteRepository<'_> {
    fn create(&self, content: &str) -> Result<Note> {
        let note = Note::new(content);

        self.conn.execute(
            "INSERT INTO notes (id, content, created_at, updated_at, is_deleted) VALUES (?, ?, ?, ?, ?)",
            params![
                note.id.as_str(),
                note.content,
                note.created_at,
                note.updated_at,
                i32::from(note.is_deleted)
            ],
        )?;

        self.sync_tags(&note.id, content)?;

        Ok(note)
    }

    fn get(&self, id: &NoteId) -> Result<Option<Note>> {
        let result = self.conn.query_row(
            "SELECT id, content, created_at, updated_at, is_deleted FROM notes WHERE id = ? AND is_deleted = 0",
            params![id.as_str()],
            Self::parse_note,
        );

        match result {
            Ok(note) => Ok(Some(note)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, created_at, updated_at, is_deleted
             FROM notes
             WHERE is_deleted = 0
             ORDER BY updated_at DESC
             LIMIT ? OFFSET ?",
        )?;

        let notes = stmt
            .query_map(params![limit as i64, offset as i64], Self::parse_note)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(notes)
    }

    fn update(&self, id: &NoteId, content: &str) -> Result<Note> {
        let now = chrono::Utc::now().timestamp_millis();

        let rows = self.conn.execute(
            "UPDATE notes SET content = ?, updated_at = ? WHERE id = ? AND is_deleted = 0",
            params![content, now, id.as_str()],
        )?;

        if rows == 0 {
            return Err(Error::NotFound(id.to_string()));
        }

        self.sync_tags(id, content)?;

        self.get(id)?.ok_or_else(|| Error::NotFound(id.to_string()))
    }

    fn delete(&self, id: &NoteId) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();

        let rows = self.conn.execute(
            "UPDATE notes SET is_deleted = 1, updated_at = ? WHERE id = ? AND is_deleted = 0",
            params![now, id.as_str()],
        )?;

        if rows == 0 {
            return Err(Error::NotFound(id.to_string()));
        }

        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<Note>> {
        if query.trim().is_empty() {
            return self.list(limit, 0);
        }

        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.content, n.created_at, n.updated_at, n.is_deleted
             FROM notes n
             JOIN notes_fts fts ON n.rowid = fts.rowid
             WHERE notes_fts MATCH ? AND n.is_deleted = 0
             ORDER BY rank
             LIMIT ?",
        )?;

        let notes = stmt
            .query_map(params![query, limit as i64], Self::parse_note)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(notes)
    }

    fn list_by_tag(&self, tag: &str, limit: usize, offset: usize) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.content, n.created_at, n.updated_at, n.is_deleted
             FROM notes n
             JOIN note_tags nt ON n.id = nt.note_id
             JOIN tags t ON nt.tag_id = t.id
             WHERE t.name = ? COLLATE NOCASE AND n.is_deleted = 0
             ORDER BY n.updated_at DESC
             LIMIT ? OFFSET ?",
        )?;

        let notes = stmt
            .query_map(params![tag, limit as i64, offset as i64], Self::parse_note)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(notes)
    }

    fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.name, COUNT(nt.note_id) as count
             FROM tags t
             LEFT JOIN note_tags nt ON t.id = nt.tag_id
             LEFT JOIN notes n ON nt.note_id = n.id AND n.is_deleted = 0
             GROUP BY t.id
             HAVING count > 0
             ORDER BY count DESC, t.name ASC",
        )?;

        let tags = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn test_create_and_get() {
        let db = setup();
        let repo = SqliteNoteRepository::new(db.connection());

        let note = repo.create("Hello world #test").unwrap();
        assert_eq!(note.content, "Hello world #test");

        let fetched = repo.get(&note.id).unwrap().unwrap();
        assert_eq!(fetched.id, note.id);
        assert_eq!(fetched.content, "Hello world #test");
    }

    #[test]
    fn test_list() {
        let db = setup();
        let repo = SqliteNoteRepository::new(db.connection());

        repo.create("Note 1").unwrap();
        repo.create("Note 2").unwrap();
        repo.create("Note 3").unwrap();

        let notes = repo.list(10, 0).unwrap();
        assert_eq!(notes.len(), 3);

        // Should be in reverse chronological order
        assert!(notes[0].created_at >= notes[1].created_at);
    }

    #[test]
    fn test_update() {
        let db = setup();
        let repo = SqliteNoteRepository::new(db.connection());

        let note = repo.create("Original").unwrap();
        let updated = repo.update(&note.id, "Updated").unwrap();

        assert_eq!(updated.content, "Updated");
        assert!(updated.updated_at >= note.updated_at);
    }

    #[test]
    fn test_delete() {
        let db = setup();
        let repo = SqliteNoteRepository::new(db.connection());

        let note = repo.create("To delete").unwrap();
        repo.delete(&note.id).unwrap();

        // Should not find deleted note
        let fetched = repo.get(&note.id).unwrap();
        assert!(fetched.is_none());

        // Should not appear in list
        let notes = repo.list(10, 0).unwrap();
        assert!(notes.is_empty());
    }

    #[test]
    fn test_search() {
        let db = setup();
        let repo = SqliteNoteRepository::new(db.connection());

        repo.create("Hello world").unwrap();
        repo.create("Goodbye world").unwrap();
        repo.create("Something else").unwrap();

        let results = repo.search("world", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_tags() {
        let db = setup();
        let repo = SqliteNoteRepository::new(db.connection());

        repo.create("Note with #rust and #programming").unwrap();
        repo.create("Another #rust note").unwrap();
        repo.create("Just #programming").unwrap();

        let tags = repo.list_tags().unwrap();
        assert_eq!(tags.len(), 2);

        // Rust should have 2 notes
        let rust_tag = tags.iter().find(|(name, _)| name == "rust").unwrap();
        assert_eq!(rust_tag.1, 2);
    }

    #[test]
    fn test_list_by_tag() {
        let db = setup();
        let repo = SqliteNoteRepository::new(db.connection());

        repo.create("Note with #rust").unwrap();
        repo.create("Another #rust note").unwrap();
        repo.create("No tag").unwrap();

        let notes = repo.list_by_tag("rust", 10, 0).unwrap();
        assert_eq!(notes.len(), 2);
    }
}
