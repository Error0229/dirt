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

    /// Test-only constructor backed by an in-process libSQL database so
    /// route + repo logic can be exercised without a real Turso target.
    ///
    /// Uses a per-test tempfile rather than `:memory:` because libsql's
    /// in-memory backend gives each new `connect()` a fresh, empty
    /// schema; `TursoRepo::conn()` opens a new connection per query,
    /// which would mean every query after `bootstrap` saw an empty DB.
    ///
    /// Wrapped by `TempDb` so the file is cleaned up on drop — without
    /// the guard, every `cargo test` invocation litters `$TMPDIR` with
    /// `.db` and `.db-wal` files.
    #[cfg(test)]
    pub async fn connect_temp_db() -> Result<TempDb, AppError> {
        let path = std::env::temp_dir().join(format!(
            "dirt-api-test-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let db = Builder::new_local(&path)
            .build()
            .await
            .map_err(|e| AppError::config(format!("failed to build local libsql: {e}")))?;
        let conn = db.connect()?;
        bootstrap(&conn).await?;
        Ok(TempDb {
            repo: std::sync::Arc::new(Self { db: Some(db) }),
            path,
        })
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

// ---- Phase 2 magic-link auth repo methods ----

/// Why a `consume_magic_code` call rejected the code. Routes layer maps
/// these onto user-facing error codes.
#[derive(Debug, PartialEq, Eq)]
pub enum ConsumeFailure {
    /// No row for that `request_id`, or `request_id` + code don't match,
    /// or the row has already been consumed. Treated identically by the
    /// caller — distinguishing them would let an attacker probe for valid
    /// `request_id`s.
    InvalidCode,
    /// Row exists but `expires_at` is past.
    Expired,
    /// `attempts >= MAX_CODE_ATTEMPTS`. The row stays in the table so a
    /// fresh `auth/request` can replace it; we don't auto-revive.
    TooManyAttempts,
}

/// Maximum failed verifications per magic-code row.
///
/// Five gives a fat-fingering user three real retries on a typo while
/// making bruteforcing 6-digit codes hopeless (1e6 / 5 ≈ 200 000 fresh
/// codes per success). Past this we stop accepting any code on the row.
pub const MAX_CODE_ATTEMPTS: i64 = 5;

impl TursoRepo {
    /// Insert a fresh magic-code row. `code_hash` is the sha256 hex of
    /// `format!("{request_id}:{code}")` — never the raw code.
    pub async fn insert_magic_code(
        &self,
        request_id: &str,
        email: &str,
        code_hash: &str,
        created_at_ms: i64,
        expires_at_ms: i64,
    ) -> Result<(), AppError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO magic_codes (request_id, email, code_hash, created_at, expires_at, consumed_at, attempts)
             VALUES (?, ?, ?, ?, ?, NULL, 0)",
            libsql::params![request_id, email, code_hash, created_at_ms, expires_at_ms],
        )
        .await?;
        Ok(())
    }

    /// Atomically check + consume a magic code. On success returns the
    /// email tied to the row (caller uses it to upsert the user). On
    /// failure returns the structured `ConsumeFailure`.
    ///
    /// The success path is a single conditional UPDATE so two concurrent
    /// verifies can't both win. The failure path issues a second SELECT
    /// only to give the user a useful error.
    pub async fn consume_magic_code(
        &self,
        request_id: &str,
        expected_code_hash: &str,
        now_ms: i64,
    ) -> Result<Result<String, ConsumeFailure>, AppError> {
        let conn = self.conn()?;

        // Fast path: a single conditional UPDATE guards every check at once.
        let affected = conn
            .execute(
                "UPDATE magic_codes
                    SET consumed_at = ?
                  WHERE request_id = ?
                    AND consumed_at IS NULL
                    AND expires_at >= ?
                    AND code_hash = ?
                    AND attempts < ?",
                libsql::params![
                    now_ms,
                    request_id,
                    now_ms,
                    expected_code_hash,
                    MAX_CODE_ATTEMPTS
                ],
            )
            .await?;

        if affected > 0 {
            let mut rows = conn
                .query(
                    "SELECT email FROM magic_codes WHERE request_id = ?",
                    libsql::params![request_id],
                )
                .await?;
            let Some(row) = rows.next().await? else {
                return Err(AppError::internal(
                    "magic code row vanished between consume and email lookup",
                ));
            };
            let email: String = row.get(0)?;
            return Ok(Ok(email));
        }

        // Failure path. Bump the attempt counter atomically *before*
        // diagnosing why the success UPDATE missed — folding the
        // "is the row still live?" predicate and the "did we fit
        // under the cap?" predicate into one statement so two
        // concurrent wrong-code requests can't both slip past the
        // attempts cap. SQLite-flavored libsql serializes write
        // statements on the same row, so the atomic predicate
        // `attempts < MAX_CODE_ATTEMPTS` is honoured exactly once
        // even under concurrent attempts.
        //
        // `affected_increment > 0` ⇒ the row was live (not consumed,
        //   not expired) and under the cap; the only way the success
        //   UPDATE could miss while this one hits is a wrong code.
        // `affected_increment == 0` ⇒ the row is in some terminal
        //   state — fall through to a SELECT to disambiguate.
        let affected_increment = conn
            .execute(
                "UPDATE magic_codes
                    SET attempts = attempts + 1
                  WHERE request_id = ?
                    AND consumed_at IS NULL
                    AND expires_at >= ?
                    AND attempts < ?",
                libsql::params![request_id, now_ms, MAX_CODE_ATTEMPTS],
            )
            .await?;

        if affected_increment > 0 {
            return Ok(Err(ConsumeFailure::InvalidCode));
        }

        // Increment missed → row is missing, consumed, expired, or locked.
        let mut rows = conn
            .query(
                "SELECT consumed_at, expires_at, attempts FROM magic_codes WHERE request_id = ?",
                libsql::params![request_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(Err(ConsumeFailure::InvalidCode));
        };
        let consumed_at: Option<i64> = row.get(0)?;
        let expires_at: i64 = row.get(1)?;
        let attempts: i64 = row.get(2)?;

        if consumed_at.is_some() {
            return Ok(Err(ConsumeFailure::InvalidCode));
        }
        if expires_at < now_ms {
            return Ok(Err(ConsumeFailure::Expired));
        }
        if attempts >= MAX_CODE_ATTEMPTS {
            return Ok(Err(ConsumeFailure::TooManyAttempts));
        }
        // Should be unreachable: increment missed for a row that's
        // live and under the cap implies a libsql transactional
        // inconsistency. Treat as an invalid code rather than
        // silently leaking a 500.
        Ok(Err(ConsumeFailure::InvalidCode))
    }

    /// Get-or-insert a user row keyed on email. Returns the canonical
    /// `user_id`. `email` must already be normalized (trimmed, lowercase).
    pub async fn upsert_user_by_email(&self, email: &str, now_ms: i64) -> Result<String, AppError> {
        let conn = self.conn()?;
        let new_id = uuid::Uuid::now_v7().to_string();
        // ON CONFLICT(email) returns the existing row's id, so the caller
        // gets a stable user_id regardless of whether this was the first
        // login or the hundredth.
        let mut rows = conn
            .query(
                "INSERT INTO users (id, email, created_at, last_login_at)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(email) DO UPDATE SET last_login_at = excluded.last_login_at
                 RETURNING id",
                libsql::params![new_id, email, now_ms, now_ms],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(AppError::internal("user upsert returned no row"));
        };
        Ok(row.get(0)?)
    }

    /// Insert a new session row and return its public `session_id` (the
    /// caller owns the raw token).
    pub async fn insert_auth_session(
        &self,
        user_id: &str,
        token_hash: &str,
        created_at_ms: i64,
        expires_at_ms: i64,
    ) -> Result<String, AppError> {
        let conn = self.conn()?;
        let session_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO auth_sessions (id, user_id, token_hash, created_at, last_used_at, expires_at, revoked_at)
             VALUES (?, ?, ?, ?, ?, ?, NULL)",
            libsql::params![
                session_id.clone(),
                user_id,
                token_hash,
                created_at_ms,
                created_at_ms,
                expires_at_ms,
            ],
        )
        .await?;
        Ok(session_id)
    }

    /// Resolve a session token (by its sha256 hash) to its session row.
    /// Returns None when the session is missing, revoked, or past
    /// `expires_at`.
    ///
    /// Bumps `last_used_at` as a side effect so the session-token TTL
    /// rolls forward with use; the row's `expires_at` is *not* touched
    /// here — refresh is the explicit way to extend a session.
    pub async fn lookup_session_by_token_hash(
        &self,
        token_hash: &str,
        now_ms: i64,
    ) -> Result<Option<SessionRow>, AppError> {
        let conn = self.conn()?;
        let mut rows = conn
            .query(
                "SELECT id, user_id, expires_at FROM auth_sessions
                  WHERE token_hash = ? AND revoked_at IS NULL AND expires_at > ?",
                libsql::params![token_hash, now_ms],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        let id: String = row.get(0)?;
        let user_id: String = row.get(1)?;
        let expires_at: i64 = row.get(2)?;

        conn.execute(
            "UPDATE auth_sessions SET last_used_at = ? WHERE id = ?",
            libsql::params![now_ms, id.clone()],
        )
        .await?;

        Ok(Some(SessionRow {
            id,
            user_id,
            expires_at_ms: expires_at,
        }))
    }

    /// Mark a session row revoked. Idempotent — re-revoking is fine.
    pub async fn revoke_session(&self, session_id: &str, now_ms: i64) -> Result<(), AppError> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE auth_sessions SET revoked_at = ?
              WHERE id = ? AND revoked_at IS NULL",
            libsql::params![now_ms, session_id],
        )
        .await?;
        Ok(())
    }
}

/// Session-row projection used by middleware + refresh handlers.
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub user_id: String,
    pub expires_at_ms: i64,
}

/// Test-only RAII handle around a tempfile-backed `TursoRepo`. Removes
/// the file (and libsql's `-shm` / `-wal` siblings, if any) on drop so
/// `cargo test` doesn't accumulate scratch DBs in `$TMPDIR`.
#[cfg(test)]
pub struct TempDb {
    pub repo: std::sync::Arc<TursoRepo>,
    path: std::path::PathBuf,
}

#[cfg(test)]
impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let mut shm = self.path.clone();
        shm.set_file_name(format!(
            "{}-shm",
            self.path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let _ = std::fs::remove_file(&shm);
        let mut wal = self.path.clone();
        wal.set_file_name(format!(
            "{}-wal",
            self.path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let _ = std::fs::remove_file(&wal);
    }
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
        // Phase 2 magic-link auth: per-user identities replace the shared
        // bearer token in Phase 1.
        "CREATE TABLE IF NOT EXISTS users (
             id TEXT PRIMARY KEY,
             email TEXT NOT NULL UNIQUE,
             created_at INTEGER NOT NULL,
             last_login_at INTEGER
         )",
        // One row per outstanding magic-code request. `code_hash` is
        // sha256(request_id || ':' || code) hex — binding the code to its
        // request_id stops a code from one request being replayed against
        // another.
        "CREATE TABLE IF NOT EXISTS magic_codes (
             request_id TEXT PRIMARY KEY,
             email TEXT NOT NULL,
             code_hash TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             expires_at INTEGER NOT NULL,
             consumed_at INTEGER,
             attempts INTEGER NOT NULL DEFAULT 0
         )",
        "CREATE INDEX IF NOT EXISTS idx_magic_codes_email_active
             ON magic_codes(email, consumed_at, expires_at)",
        // `token_hash` is sha256 of the random session token. We never
        // store the raw token. `revoked_at` doubles as the logout marker.
        "CREATE TABLE IF NOT EXISTS auth_sessions (
             id TEXT PRIMARY KEY,
             user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
             token_hash TEXT NOT NULL UNIQUE,
             created_at INTEGER NOT NULL,
             last_used_at INTEGER NOT NULL,
             expires_at INTEGER NOT NULL,
             revoked_at INTEGER
         )",
        "CREATE INDEX IF NOT EXISTS idx_auth_sessions_user
             ON auth_sessions(user_id, revoked_at, expires_at)",
        "CREATE INDEX IF NOT EXISTS idx_auth_sessions_token
             ON auth_sessions(token_hash)",
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
