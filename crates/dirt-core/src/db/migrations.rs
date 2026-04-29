//! Database migrations

use crate::error::Result;
use crate::SOLO_USER_ID;
use libsql::Connection;

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
    if version < 4 {
        migrate_v4(conn).await?;
    }
    if version < 5 {
        migrate_v5(conn).await?;
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

    tracing::info!("Migrated database to version 2");
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

    tracing::info!("Migrated database to version 3");
    Ok(())
}

/// Migration to version 4: server-authoritative sync scaffolding.
///
/// Ordering matters because FTS triggers reference `notes.content` and the
/// `idx_notes_deleted` index references `is_deleted`, which we're about to
/// drop. Steps:
///
/// 1. Drop the three existing FTS triggers (`notes_ai`, `notes_au`, `notes_ad`).
/// 2. Drop `idx_notes_deleted` before removing the `is_deleted` column.
/// 3. Add `user_id`, `server_updated_at`, `deleted_at` columns.
/// 4. Backfill `deleted_at` from any rows with `is_deleted = 1`, using
///    `updated_at` as the approximate tombstone moment.
/// 5. Drop the `is_deleted` column (requires `SQLite` ≥ 3.35, satisfied by
///    libsql 0.9's bundled 3.44+).
/// 6. Recreate the FTS triggers filtering on `deleted_at IS NULL` so full-text
///    search ignores tombstoned notes.
/// 7. Add `idx_notes_deleted_at` and `idx_notes_sua` for the new query paths.
/// 8. Create `pending_sync` and `sync_state` tables the client-side sync
///    worker will read and write.
/// 9. Backfill `pending_sync` from every existing row in `notes`. Pre-v4
///    rows have no `server_updated_at` and have never been seen by the new
///    backend, so they all need to be queued for first push (live notes
///    *and* tombstones — the server has to learn about deletions too).
///    Without this step, upgraded databases would silently leave all
///    historical notes unsynced until the user re-edited them.
///
/// FTS purge note: pre-v4 the FTS triggers had no `deleted_at` filter,
/// so `notes_fts` indexed tombstones too. We do *not* try to scrub them
/// here — the FTS5 external-content + libsql transaction interaction
/// produces `SQLITE_LOCKED` on every approach we tried (delete-all +
/// re-insert, no-op self-UPDATE to fire the new trigger, rebuild). The
/// search query already filters `deleted_at IS NULL`, so phantom rows
/// stay invisible to users; any future code path that joins FTS *must*
/// repeat that guard. If FTS ever needs to be authoritative on its own,
/// do the cleanup in app-side code outside the migration transaction.
async fn migrate_v4(conn: &Connection) -> Result<()> {
    conn.execute("BEGIN TRANSACTION", ()).await?;

    let add_user_id =
        format!("ALTER TABLE notes ADD COLUMN user_id TEXT NOT NULL DEFAULT '{SOLO_USER_ID}'");

    let statements: [&str; 18] = [
        // 1. Drop old FTS triggers before touching notes.
        "DROP TRIGGER IF EXISTS notes_ai",
        "DROP TRIGGER IF EXISTS notes_au",
        "DROP TRIGGER IF EXISTS notes_ad",
        // 2. Drop the is_deleted-based index so the column drop can proceed.
        "DROP INDEX IF EXISTS idx_notes_deleted",
        // 3. New columns.
        add_user_id.as_str(),
        "ALTER TABLE notes ADD COLUMN server_updated_at INTEGER",
        "ALTER TABLE notes ADD COLUMN deleted_at INTEGER",
        // 4. Port is_deleted -> deleted_at. Use updated_at as approximate moment.
        "UPDATE notes SET deleted_at = updated_at WHERE is_deleted = 1",
        // 5. Drop the legacy boolean column.
        "ALTER TABLE notes DROP COLUMN is_deleted",
        // 6. Recreate FTS triggers with deleted_at filtering.
        "CREATE TRIGGER IF NOT EXISTS notes_ai AFTER INSERT ON notes
            WHEN NEW.deleted_at IS NULL
         BEGIN
             INSERT INTO notes_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
         END",
        "CREATE TRIGGER IF NOT EXISTS notes_ad AFTER DELETE ON notes BEGIN
             INSERT INTO notes_fts(notes_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
         END",
        "CREATE TRIGGER IF NOT EXISTS notes_au AFTER UPDATE ON notes BEGIN
             INSERT INTO notes_fts(notes_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
             INSERT INTO notes_fts(rowid, content)
                 SELECT NEW.rowid, NEW.content WHERE NEW.deleted_at IS NULL;
         END",
        // 7. New indices.
        "CREATE INDEX IF NOT EXISTS idx_notes_deleted_at ON notes(deleted_at)",
        "CREATE INDEX IF NOT EXISTS idx_notes_sua ON notes(user_id, server_updated_at)",
        // 8. Sync scaffolding tables.
        "CREATE TABLE IF NOT EXISTS pending_sync (
             user_id TEXT NOT NULL,
             note_id TEXT NOT NULL,
             dirty_at INTEGER NOT NULL,
             PRIMARY KEY (user_id, note_id)
         )",
        "CREATE TABLE IF NOT EXISTS sync_state (
             user_id TEXT PRIMARY KEY,
             cursor_sua INTEGER NOT NULL DEFAULT 0,
             cursor_id TEXT
         )",
        // 9. Queue every pre-v4 note for first push.
        "INSERT INTO pending_sync (user_id, note_id, dirty_at)
             SELECT user_id, id, updated_at FROM notes",
        "INSERT INTO schema_version (version) VALUES (4)",
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

    tracing::info!("Migrated database to version 4");
    Ok(())
}

/// Migration to version 5: drop legacy LWW-trigger and managed-SaaS tables.
///
/// The `notes_lww_conflict_guard` trigger silently `RAISE(IGNORE)`s any UPDATE
/// where `NEW.updated_at < OLD.updated_at`, which is correct for the old
/// same-device LWW model but fatally wrong for pull-merge: stale-by-client-clock
/// pulls would be dropped without merging. The conflict log and attachment
/// tables go with it — Phase 1 has no media path and no conflict log concept.
async fn migrate_v5(conn: &Connection) -> Result<()> {
    conn.execute("BEGIN TRANSACTION", ()).await?;

    let statements = [
        "DROP TRIGGER IF EXISTS notes_lww_conflict_guard",
        "DROP INDEX IF EXISTS idx_sync_conflicts_note_id",
        "DROP INDEX IF EXISTS idx_sync_conflicts_resolved_at",
        "DROP TABLE IF EXISTS sync_conflicts",
        "DROP INDEX IF EXISTS idx_attachments_note_id",
        "DROP INDEX IF EXISTS idx_attachments_created_at",
        "DROP INDEX IF EXISTS idx_attachments_deleted",
        "DROP TABLE IF EXISTS attachments",
        "INSERT INTO schema_version (version) VALUES (5)",
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

    tracing::info!("Migrated database to version 5");
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

    async fn table_exists(conn: &Connection, name: &str) -> bool {
        let mut rows = conn
            .query(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
                [name],
            )
            .await
            .unwrap();
        rows.next()
            .await
            .unwrap()
            .is_some_and(|row| row.get::<i32>(0).unwrap() != 0)
    }

    async fn trigger_exists(conn: &Connection, name: &str) -> bool {
        let mut rows = conn
            .query(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = ?)",
                [name],
            )
            .await
            .unwrap();
        rows.next()
            .await
            .unwrap()
            .is_some_and(|row| row.get::<i32>(0).unwrap() != 0)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrations() {
        let conn = setup().await;
        run(&conn).await.unwrap();

        let version = get_version(&conn).await.unwrap();
        assert_eq!(version, 5);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrations_idempotent() {
        let conn = setup().await;
        run(&conn).await.unwrap();
        run(&conn).await.unwrap(); // Should not fail

        let version = get_version(&conn).await.unwrap();
        assert_eq!(version, 5);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrate_v5_drops_legacy_tables_and_trigger() {
        let conn = setup().await;
        run(&conn).await.unwrap();

        assert!(!table_exists(&conn, "attachments").await);
        assert!(!table_exists(&conn, "sync_conflicts").await);
        assert!(!trigger_exists(&conn, "notes_lww_conflict_guard").await);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrate_v4_from_v3_with_data() {
        // Build a v3-shape DB by running v1..v3 only.
        let conn = setup().await;
        migrate_v1(&conn).await.unwrap();
        migrate_v2(&conn).await.unwrap();
        migrate_v3(&conn).await.unwrap();
        assert_eq!(get_version(&conn).await.unwrap(), 3);

        // Seed v3-shape data, including a tombstoned row the migration must port.
        let live_id = "01932aaa-0000-7000-8000-000000000001";
        let dead_id = "01932aaa-0000-7000-8000-000000000002";
        let dead_ts: i64 = 1_700_000_000_000;

        conn.execute(
            "INSERT INTO notes (id, content, created_at, updated_at, is_deleted)
             VALUES (?, ?, ?, ?, 0)",
            libsql::params![
                live_id,
                "live content #tag",
                1_699_999_000_000_i64,
                1_699_999_500_000_i64
            ],
        )
        .await
        .unwrap();
        conn.execute(
            "INSERT INTO notes (id, content, created_at, updated_at, is_deleted)
             VALUES (?, ?, ?, ?, 1)",
            libsql::params![dead_id, "dead content", 1_699_999_000_000_i64, dead_ts],
        )
        .await
        .unwrap();

        // Also seed legacy tables that v5 drops — must not block the migration.
        conn.execute(
            "INSERT INTO sync_conflicts (note_id, local_updated_at, incoming_updated_at, resolved_at, strategy)
             VALUES ('x', 1, 2, 3, 'lww')",
            (),
        )
        .await
        .unwrap();
        conn.execute(
            "INSERT INTO attachments (id, note_id, filename, mime_type, size_bytes, r2_key, created_at, is_deleted)
             VALUES ('att-1', ?, 'f.png', 'image/png', 10, 'notes/f.png', 1, 0)",
            [live_id],
        )
        .await
        .unwrap();

        // Now run the full pipeline forward. Expected final version: 5.
        run(&conn).await.unwrap();
        assert_eq!(get_version(&conn).await.unwrap(), 5);

        // Live row: user_id defaulted, deleted_at IS NULL, server_updated_at IS NULL.
        let mut rows = conn
            .query(
                "SELECT user_id, deleted_at, server_updated_at FROM notes WHERE id = ?",
                [live_id],
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let user_id: String = row.get(0).unwrap();
        let deleted_at: Option<i64> = row.get(1).unwrap();
        let server_updated_at: Option<i64> = row.get(2).unwrap();
        assert_eq!(user_id, SOLO_USER_ID);
        assert!(deleted_at.is_none());
        assert!(server_updated_at.is_none());

        // Tombstoned row: deleted_at == its original updated_at.
        let mut rows = conn
            .query("SELECT deleted_at FROM notes WHERE id = ?", [dead_id])
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let deleted_at: Option<i64> = row.get(0).unwrap();
        assert_eq!(deleted_at, Some(dead_ts));

        // Legacy tables gone.
        assert!(!table_exists(&conn, "attachments").await);
        assert!(!table_exists(&conn, "sync_conflicts").await);
        assert!(!trigger_exists(&conn, "notes_lww_conflict_guard").await);

        // New sync tables present.
        assert!(table_exists(&conn, "pending_sync").await);
        assert!(table_exists(&conn, "sync_state").await);

        // pending_sync was backfilled from existing notes — both the live
        // and the tombstoned row need to be queued for first push.
        let mut rows = conn
            .query("SELECT COUNT(*) FROM pending_sync", ())
            .await
            .unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(
            count, 2,
            "pending_sync should be backfilled with both notes"
        );

        // Confirm each note id is present, paired with the right dirty_at
        // (the row's pre-migration `updated_at`).
        let mut rows = conn
            .query(
                "SELECT dirty_at FROM pending_sync WHERE note_id = ?",
                [live_id],
            )
            .await
            .unwrap();
        let live_dirty: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(live_dirty, 1_699_999_500_000_i64);

        let mut rows = conn
            .query(
                "SELECT dirty_at FROM pending_sync WHERE note_id = ?",
                [dead_id],
            )
            .await
            .unwrap();
        let dead_dirty: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(dead_dirty, dead_ts);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrate_v4_fts_triggers_survive() {
        let conn = setup().await;
        run(&conn).await.unwrap();

        // Live note should appear in FTS.
        let live_id = "01932aaa-0000-7000-8000-0000000000aa";
        conn.execute(
            "INSERT INTO notes (id, user_id, content, created_at, updated_at, server_updated_at, deleted_at)
             VALUES (?, ?, 'findable apple', 1, 1, NULL, NULL)",
            libsql::params![live_id, SOLO_USER_ID],
        )
        .await
        .unwrap();

        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM notes n JOIN notes_fts f ON n.rowid = f.rowid WHERE notes_fts MATCH 'apple'",
                (),
            )
            .await
            .unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(count, 1, "live note must appear in FTS");

        // Tombstoning removes it from FTS via the updated AU trigger.
        conn.execute("UPDATE notes SET deleted_at = 2 WHERE id = ?", [live_id])
            .await
            .unwrap();

        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM notes n JOIN notes_fts f ON n.rowid = f.rowid WHERE notes_fts MATCH 'apple'",
                (),
            )
            .await
            .unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(count, 0, "tombstoned note must be absent from FTS");

        // A note inserted already-tombstoned should never hit FTS thanks to the WHEN clause.
        let born_dead = "01932aaa-0000-7000-8000-0000000000bb";
        conn.execute(
            "INSERT INTO notes (id, user_id, content, created_at, updated_at, server_updated_at, deleted_at)
             VALUES (?, ?, 'stillborn banana', 1, 1, NULL, 10)",
            libsql::params![born_dead, SOLO_USER_ID],
        )
        .await
        .unwrap();

        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM notes n JOIN notes_fts f ON n.rowid = f.rowid WHERE notes_fts MATCH 'banana'",
                (),
            )
            .await
            .unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(count, 0, "note inserted tombstoned must skip FTS");
    }
}
