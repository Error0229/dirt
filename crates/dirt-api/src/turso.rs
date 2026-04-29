//! Server-side libSQL client and schema bootstrap.
//!
//! One long-lived `libsql::Database` per process. `bootstrap` creates the
//! server schema on first run, matching the shape in the design doc. Push
//! and pull both stamp `server_updated_at` with `now_ms()` — clients never
//! write that column.

use std::sync::Arc;

use dirt_core::models::{Note, NoteId};
use libsql::{Builder, Connection, Database};

use crate::error::AppError;

/// Maximum notes accepted in one push batch.
pub const PUSH_BATCH_LIMIT: usize = 500;

/// Default page size for pulls. Callers may override via query string up to
/// `PULL_MAX_LIMIT`.
pub const PULL_DEFAULT_LIMIT: usize = 500;

/// Hard ceiling to bound memory even if a malicious client asks for more.
pub const PULL_MAX_LIMIT: usize = 1000;

pub struct TursoRepo {
    db: Option<Database>,
}

impl TursoRepo {
    /// Connect to a Turso remote database and run the server-side schema
    /// bootstrap. Idempotent: running twice against a seeded DB is a no-op.
    pub async fn connect(url: &str, auth_token: &str) -> Result<Self, AppError> {
        let db = Builder::new_remote(url.to_string(), auth_token.to_string())
            .build()
            .await
            .map_err(|e| AppError::config(format!("failed to build Turso client: {e}")))?;
        let conn = db.connect()?;
        bootstrap(&conn).await?;
        Ok(Self { db: Some(db) })
    }

    /// Test-only constructor that holds no real database. Any handler that
    /// actually queries will panic — only useful for middleware tests that
    /// don't touch the repo.
    #[cfg(test)]
    pub const fn dangling() -> Self {
        Self { db: None }
    }

    fn conn(&self) -> Result<Connection, AppError> {
        let db = self
            .db
            .as_ref()
            .ok_or_else(|| AppError::internal("TursoRepo used without a live connection"))?;
        db.connect().map_err(Into::into)
    }

    /// Upsert one note and stamp `server_updated_at` on the server clock.
    ///
    /// Returns the stamped `server_updated_at_ms` so the caller can include
    /// it in the per-note result the client needs to update its local
    /// `server_updated_at` column and clear its `pending_sync` entry.
    pub async fn upsert(
        &self,
        user_id: &str,
        note: &PushNote<'_>,
        server_now_ms: i64,
    ) -> Result<i64, AppError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO notes (id, user_id, content, created_at, client_updated_at, server_updated_at, deleted_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                 user_id = excluded.user_id,
                 content = excluded.content,
                 created_at = excluded.created_at,
                 client_updated_at = excluded.client_updated_at,
                 server_updated_at = excluded.server_updated_at,
                 deleted_at = excluded.deleted_at",
            libsql::params![
                note.id.as_str(),
                user_id,
                note.content,
                note.created_at_ms,
                note.client_updated_at_ms,
                server_now_ms,
                note.deleted_at_ms,
            ],
        )
        .await?;
        Ok(server_now_ms)
    }

    /// Fetch a page of notes strictly after `(cursor_sua, cursor_id)` for a
    /// given user. Ordering uses `(server_updated_at, id)` to guarantee a
    /// total order even when multiple rows share the same timestamp.
    pub async fn pull_page(
        &self,
        user_id: &str,
        cursor_sua: i64,
        cursor_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Note>, AppError> {
        let conn = self.conn()?;
        // Defensive backstop. The routes layer clamps user input to
        // PULL_MAX_LIMIT and adds 1 as a "is there more?" probe row, so
        // we accept up to PULL_MAX_LIMIT + 1 here. Anything wilder
        // collapses back to the cap.
        let probe_ceiling = PULL_MAX_LIMIT.saturating_add(1);
        let clamped_limit =
            i64::try_from(limit.clamp(1, probe_ceiling)).expect("probe_ceiling always fits in i64");

        let mut rows = if let Some(id) = cursor_id {
            conn.query(
                "SELECT id, user_id, content, created_at, client_updated_at, server_updated_at, deleted_at
                 FROM notes
                 WHERE user_id = ?
                   AND (server_updated_at > ?
                        OR (server_updated_at = ? AND id > ?))
                 ORDER BY server_updated_at ASC, id ASC
                 LIMIT ?",
                libsql::params![user_id, cursor_sua, cursor_sua, id, clamped_limit],
            )
            .await?
        } else {
            conn.query(
                "SELECT id, user_id, content, created_at, client_updated_at, server_updated_at, deleted_at
                 FROM notes
                 WHERE user_id = ? AND server_updated_at > ?
                 ORDER BY server_updated_at ASC, id ASC
                 LIMIT ?",
                libsql::params![user_id, cursor_sua, clamped_limit],
            )
            .await?
        };

        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(parse_pulled_note(&row)?);
        }
        Ok(out)
    }
}

/// Body shape the push handler already validated. Kept here so the repo
/// stays oblivious to JSON concerns.
pub struct PushNote<'a> {
    pub id: &'a NoteId,
    pub content: &'a str,
    pub created_at_ms: i64,
    pub client_updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

fn parse_pulled_note(row: &libsql::Row) -> Result<Note, AppError> {
    let id: String = row.get(0)?;
    let id = id
        .parse::<NoteId>()
        .map_err(|_| AppError::internal(format!("invalid note id stored on server: {id}")))?;
    Ok(Note {
        id,
        user_id: row.get(1)?,
        content: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get::<i64>(4)?,
        server_updated_at: row.get(5)?,
        deleted_at: row.get(6)?,
    })
}

async fn bootstrap(conn: &Connection) -> Result<(), AppError> {
    // No transaction — CREATE TABLE IF NOT EXISTS is idempotent and each
    // statement can be retried independently. libsql's remote driver does
    // not pipeline DDL usefully anyway.
    let statements = [
        "CREATE TABLE IF NOT EXISTS notes (
             id TEXT PRIMARY KEY,
             user_id TEXT NOT NULL,
             content TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             client_updated_at INTEGER NOT NULL,
             server_updated_at INTEGER NOT NULL,
             deleted_at INTEGER
         )",
        "CREATE INDEX IF NOT EXISTS idx_notes_user_sua ON notes(user_id, server_updated_at, id)",
    ];
    for stmt in statements {
        conn.execute(stmt, ()).await?;
    }
    Ok(())
}

/// Wrap into `Arc` for the handler state. Keeps every handler reference
/// cheap to clone.
#[must_use]
pub fn arc(repo: TursoRepo) -> Arc<TursoRepo> {
    Arc::new(repo)
}
