//! Database migrations

use crate::error::Result;
use libsql::Connection;

/// Current schema version
const CURRENT_VERSION: i32 = 3;

/// Run all pending migrations
pub async fn run(conn: &Connection) -> Result<()> {
    let version = get_version(conn).await?;

    if version < 1 {
        migrate_v1(conn).await?;
    }
    if version < 2 {
        migrate_v2(conn).await?;
    }
    if version < 3 {
        migrate_v3(conn).await?;
    }

    Ok(())
}

/// Get the current schema version
async fn get_version(conn: &Connection) -> Result<i32> {
    // Check if schema_version table exists
    let mut rows = conn
        .query(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version')",
            (),
        )
        .await?;

    let exists: bool = if let Some(row) = rows.next().await? {
        row.get::<i32>(0)? != 0
    } else {
        false
    };

    if !exists {
        return Ok(0);
    }

    let mut rows = conn
        .query("SELECT COALESCE(MAX(version), 0) FROM schema_version", ())
        .await?;

    let version: i32 = if let Some(row) = rows.next().await? {
        row.get(0)?
    } else {
        0
    };

    Ok(version)
}

/// Migration to version 1: Initial schema
async fn migrate_v1(conn: &Connection) -> Result<()> {
    // libsql doesn't have execute_batch, so we run each statement separately
    // Using a transaction for atomicity

    conn.execute("BEGIN TRANSACTION", ()).await?;

    let statements = [
        // Schema version tracking
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        )",
        // Notes table
        "CREATE TABLE IF NOT EXISTS notes (
            id TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            is_deleted INTEGER NOT NULL DEFAULT 0
        )",
        "CREATE INDEX IF NOT EXISTS idx_notes_updated ON notes(updated_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_notes_created ON notes(created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_notes_deleted ON notes(is_deleted)",
        // Tags table
        "CREATE TABLE IF NOT EXISTS tags (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE COLLATE NOCASE,
            created_at INTEGER NOT NULL
        )",
        // Note-Tag junction table
        "CREATE TABLE IF NOT EXISTS note_tags (
            note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (note_id, tag_id)
        )",
        "CREATE INDEX IF NOT EXISTS idx_note_tags_tag ON note_tags(tag_id)",
        // Full-text search
        "CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
            content,
            content=notes,
            content_rowid=rowid
        )",
        // Triggers to keep FTS in sync
        "CREATE TRIGGER IF NOT EXISTS notes_ai AFTER INSERT ON notes BEGIN
            INSERT INTO notes_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
        END",
        "CREATE TRIGGER IF NOT EXISTS notes_ad AFTER DELETE ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
        END",
        "CREATE TRIGGER IF NOT EXISTS notes_au AFTER UPDATE ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
            INSERT INTO notes_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
        END",
        // Settings table (local only)
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        // Record migration version
        "INSERT INTO schema_version (version) VALUES (1)",
    ];

    for stmt in statements {
        if let Err(e) = conn.execute(stmt, ()).await {
            conn.execute("ROLLBACK", ()).await.ok();
            return Err(e.into());
        }
    }

    if let Err(e) = conn.execute("COMMIT", ()).await {
        conn.execute("ROLLBACK", ()).await.ok();
        return Err(e.into());
    }

    tracing::info!("Migrated database to version 1");
    Ok(())
}

/// Migration to version 2: LWW conflict logging support
async fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute("BEGIN TRANSACTION", ()).await?;

    let statements = [
        "CREATE TABLE IF NOT EXISTS sync_conflicts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id TEXT NOT NULL,
            local_updated_at INTEGER NOT NULL,
            incoming_updated_at INTEGER NOT NULL,
            resolved_at INTEGER NOT NULL,
            strategy TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_sync_conflicts_note_id ON sync_conflicts(note_id)",
        "CREATE INDEX IF NOT EXISTS idx_sync_conflicts_resolved_at ON sync_conflicts(resolved_at DESC)",
        "CREATE TRIGGER IF NOT EXISTS notes_lww_conflict_guard BEFORE UPDATE ON notes
         FOR EACH ROW
         WHEN NEW.updated_at < OLD.updated_at
         BEGIN
             INSERT INTO sync_conflicts (
                 note_id,
                 local_updated_at,
                 incoming_updated_at,
                 resolved_at,
                 strategy
             ) VALUES (
                 OLD.id,
                 OLD.updated_at,
                 NEW.updated_at,
                 CAST(strftime('%s','now') AS INTEGER) * 1000,
                 'lww'
             );
             SELECT RAISE(IGNORE);
         END",
        "INSERT INTO schema_version (version) VALUES (2)",
    ];

    for stmt in statements {
        if let Err(e) = conn.execute(stmt, ()).await {
            conn.execute("ROLLBACK", ()).await.ok();
            return Err(e.into());
        }
    }

    if let Err(e) = conn.execute("COMMIT", ()).await {
        conn.execute("ROLLBACK", ()).await.ok();
        return Err(e.into());
    }

    tracing::info!("Migrated database to version {CURRENT_VERSION}");
    Ok(())
}

/// Migration to version 3: Attachment metadata support
async fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute("BEGIN TRANSACTION", ()).await?;

    let statements = [
        "CREATE TABLE IF NOT EXISTS attachments (
            id TEXT PRIMARY KEY,
            note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            filename TEXT NOT NULL,
            mime_type TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            r2_key TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            is_deleted INTEGER NOT NULL DEFAULT 0
        )",
        "CREATE INDEX IF NOT EXISTS idx_attachments_note_id ON attachments(note_id)",
        "CREATE INDEX IF NOT EXISTS idx_attachments_created_at ON attachments(created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_attachments_deleted ON attachments(is_deleted)",
        "INSERT INTO schema_version (version) VALUES (3)",
    ];

    for stmt in statements {
        if let Err(e) = conn.execute(stmt, ()).await {
            conn.execute("ROLLBACK", ()).await.ok();
            return Err(e.into());
        }
    }

    if let Err(e) = conn.execute("COMMIT", ()).await {
        conn.execute("ROLLBACK", ()).await.ok();
        return Err(e.into());
    }

    tracing::info!("Migrated database to version {CURRENT_VERSION}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use libsql::Builder;

    async fn setup() -> Connection {
        let db = Builder::new_local(":memory:").build().await.unwrap();
        db.connect().unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrations() {
        let conn = setup().await;
        run(&conn).await.unwrap();

        let version = get_version(&conn).await.unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrations_idempotent() {
        let conn = setup().await;
        run(&conn).await.unwrap();
        run(&conn).await.unwrap(); // Should not fail

        let version = get_version(&conn).await.unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migration_v3_creates_attachments_table() {
        let conn = setup().await;
        run(&conn).await.unwrap();

        let mut rows = conn
            .query(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master
                    WHERE type = 'table' AND name = 'attachments'
                )",
                (),
            )
            .await
            .unwrap();

        let exists = rows
            .next()
            .await
            .unwrap()
            .is_some_and(|row| row.get::<i32>(0).unwrap() != 0);

        assert!(exists);
    }
}
