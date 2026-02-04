//! Database migrations

use crate::error::Result;
use rusqlite::Connection;

/// Current schema version
const CURRENT_VERSION: i32 = 1;

/// Run all pending migrations
pub fn run(conn: &mut Connection) -> Result<()> {
    let version = get_version(conn)?;

    if version < 1 {
        migrate_v1(conn)?;
    }

    Ok(())
}

/// Get the current schema version
fn get_version(conn: &Connection) -> Result<i32> {
    // Check if schema_version table exists
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version')",
        [],
        |row| row.get(0),
    )?;

    if !exists {
        return Ok(0);
    }

    let version: i32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;

    Ok(version)
}

/// Migration to version 1: Initial schema
fn migrate_v1(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;

    tx.execute_batch(
        r"
        -- Schema version tracking
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );

        -- Notes table
        CREATE TABLE IF NOT EXISTS notes (
            id TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            is_deleted INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_notes_updated ON notes(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_notes_created ON notes(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_notes_deleted ON notes(is_deleted);

        -- Tags table
        CREATE TABLE IF NOT EXISTS tags (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE COLLATE NOCASE,
            created_at INTEGER NOT NULL
        );

        -- Note-Tag junction table
        CREATE TABLE IF NOT EXISTS note_tags (
            note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (note_id, tag_id)
        );

        CREATE INDEX IF NOT EXISTS idx_note_tags_tag ON note_tags(tag_id);

        -- Full-text search
        CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
            content,
            content=notes,
            content_rowid=rowid
        );

        -- Triggers to keep FTS in sync
        CREATE TRIGGER IF NOT EXISTS notes_ai AFTER INSERT ON notes BEGIN
            INSERT INTO notes_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
        END;

        CREATE TRIGGER IF NOT EXISTS notes_ad AFTER DELETE ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
        END;

        CREATE TRIGGER IF NOT EXISTS notes_au AFTER UPDATE ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
            INSERT INTO notes_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
        END;

        -- Settings table (local only)
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        -- Record migration version
        INSERT INTO schema_version (version) VALUES (1);
        ",
    )?;

    tx.commit()?;
    tracing::info!("Migrated database to version {CURRENT_VERSION}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();

        let version = get_version(&conn).unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }

    #[test]
    fn test_migrations_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap(); // Should not fail

        let version = get_version(&conn).unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }
}
