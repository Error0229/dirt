//! Per-user local DB path resolution + the legacy-to-per-user
//! migration that runs on first authenticated sign-in.
//!
//! Phase 2.x layout — see `docs/plans/2026-05-26-issue-234-per-user-db-partitioning.md`:
//!
//! ```text
//! <data_dir>/                           e.g. ~/.local/share/dirt
//! ├── state.json                        {"active_user_id": "<uuid>"} or absent
//! ├── dirt.db                           pre-first-signin SOLO DB only
//! └── <user_id>/
//!     └── dirt.db                       per-user DB after sign-in
//! ```
//!
//! The active-user pointer is the source of truth for "which DB does
//! this machine use right now," and it survives sign-out. After the
//! first sign-in on a machine the legacy top-level `dirt.db` is gone
//! forever (it's been migrated into the user's directory and its
//! rows' `user_id` columns rewritten from `SOLO_USER_ID` to the
//! signed-in user's id).
//!
//! This module is the single seam every binary calls through —
//! desktop / mobile / CLI all share these helpers so the on-disk
//! layout never drifts between them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use crate::db::Database;
use crate::error::{Error, Result};
use crate::SOLO_USER_ID;

/// Filename of the active-user pointer at the top of the dirt data
/// directory.
const STATE_FILENAME: &str = "state.json";
/// Default `SQLite` filename used by desktop and CLI.
///
/// Mobile uses a different name ([`MOBILE_DB_FILENAME`]) so every
/// path helper that touches the filesystem takes the filename as a
/// parameter rather than hardcoding it.
pub const DB_FILENAME: &str = "dirt.db";

/// Filename used by the Android shell.
///
/// Distinct from [`DB_FILENAME`] so a developer who pointed both
/// desktop and mobile at the same `<data_dir>` for debugging doesn't
/// have them stomp on each other's row layout. Mobile callers thread
/// this in at the call site so the migration finds and moves the
/// correct legacy file.
pub const MOBILE_DB_FILENAME: &str = "dirt-mobile.db";

/// Persistent state at the top of the dirt data directory.
///
/// Kept narrow so future fields (other-known-accounts list, last
/// active timestamp, etc.) can be added without breaking older
/// builds. `serde` rejects unknown fields by default; we use the
/// default behavior intentionally to surface schema drift loudly
/// rather than silently lose data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct State {
    active_user_id: String,
}

/// Outcome of a [`migrate_solo_db_to_user`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoloMigrationOutcome {
    /// Neither the legacy solo DB nor a per-user DB existed for the
    /// given user. Nothing to do.
    NoSoloDb,
    /// Legacy `dirt.db` was moved into `<user_id>/` and the rows'
    /// `user_id` columns were rewritten from `SOLO_USER_ID`.
    Migrated,
    /// The per-user DB already exists; the legacy DB is gone. Treated
    /// as a no-op. Any leftover `SOLO_USER_ID` rows inside the
    /// per-user DB get rewritten as a safety net so a partial
    /// previous run doesn't leave a half-stamped DB.
    AlreadyMigrated,
}

/// Legacy single-DB path used pre-Phase-2 and for the first launch on
/// a brand-new machine that has never signed in.
///
/// `db_filename` is one of [`DB_FILENAME`] (desktop / CLI) or
/// [`MOBILE_DB_FILENAME`]. Passing it explicitly at every call site
/// makes the desktop/mobile filename divergence visible — a previous
/// version hardcoded `dirt.db` and silently skipped the mobile
/// migration because the legacy file was actually `dirt-mobile.db`.
#[must_use]
pub fn solo_db_path(data_dir: &Path, db_filename: &str) -> PathBuf {
    data_dir.join(db_filename)
}

/// Per-user DB path for `user_id`.
///
/// `user_id` must be pre-validated by [`validate_user_id`]; callers
/// reach this only after the boundary check. See [`solo_db_path`]
/// for the `db_filename` convention.
pub fn user_db_path(data_dir: &Path, user_id: &str, db_filename: &str) -> Result<PathBuf> {
    let safe = validate_user_id(user_id)?;
    Ok(data_dir.join(safe).join(db_filename))
}

/// Validate that `user_id` is a UUID-v7-shaped string and safe to use
/// as a filesystem segment.
///
/// Rejects empty strings, anything containing path separators, `..`,
/// NUL, or anything `uuid` itself rejects. Returns the input on
/// success so the caller can chain.
///
/// The server stamps `user_id` as a UUID v7 in every payload that
/// reaches the clients — keyring `StoredToken.user_id`, `/v1/notes/pull`
/// responses, etc. — so this is the same shape the rest of the system
/// already validates. Rejecting non-UUID strings here closes the
/// "what if a buggy proxy injects `../../etc/passwd` into the `user_id`
/// field" path before it touches the filesystem.
pub fn validate_user_id(user_id: &str) -> Result<&str> {
    if user_id.is_empty() {
        return Err(Error::InvalidInput("user_id must not be empty".into()));
    }
    if user_id.contains('/') || user_id.contains('\\') || user_id.contains('\0') {
        return Err(Error::InvalidInput(format!(
            "user_id {user_id:?} contains a path separator or NUL"
        )));
    }
    if user_id == "." || user_id == ".." {
        return Err(Error::InvalidInput(format!(
            "user_id must not be {user_id:?}"
        )));
    }
    // UUID v7 is the only shape the API ever stamps; reject anything
    // else loudly so a typo (or a malicious payload) never lands as a
    // directory name. This also rejects the all-zero UUID (Uuid parses
    // it but it has no meaningful identity).
    Uuid::parse_str(user_id)
        .map_err(|err| Error::InvalidInput(format!("user_id {user_id:?} is not a UUID: {err}")))?;
    Ok(user_id)
}

/// Path to the active-user pointer file.
#[must_use]
pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(STATE_FILENAME)
}

/// Read the active-user pointer. `Ok(None)` if absent.
///
/// `Err` only for the cases the caller can't reasonably recover from —
/// I/O failures other than `NotFound`, or a parse error indicating a
/// corrupt / future-version state file. (Per CLAUDE.md: surface, don't
/// silently fall back to solo.)
pub async fn read_active_user(data_dir: &Path) -> Result<Option<String>> {
    let path = state_path(data_dir);
    let bytes = match fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };
    let state: State = serde_json::from_slice(&bytes).map_err(|err| {
        Error::InvalidInput(format!(
            "{} is unreadable ({err}); delete or restore it manually",
            path.display()
        ))
    })?;
    // Defensive: a state file pointing at a malformed user_id is a
    // data-corruption signal, not "fall back to solo." Surface it.
    validate_user_id(&state.active_user_id)?;
    Ok(Some(state.active_user_id))
}

/// Atomically rewrite the active-user pointer.
///
/// Write-temp + fsync + rename so a power-cut never leaves a
/// half-written `state.json`. Safe to call concurrently across
/// CLI / desktop / mobile processes — last writer wins, which
/// matches the "whichever client signed in last is the active user"
/// semantics.
pub async fn write_active_user(data_dir: &Path, user_id: &str) -> Result<()> {
    validate_user_id(user_id)?;
    fs::create_dir_all(data_dir).await.map_err(Error::Io)?;

    let state = State {
        active_user_id: user_id.to_string(),
    };
    let body = serde_json::to_vec_pretty(&state)?;

    let final_path = state_path(data_dir);
    // Temp file lives alongside the final path so rename is same-FS.
    // Suffix is process-unique to avoid clobbering a parallel write.
    let tmp_path = data_dir.join(format!("{STATE_FILENAME}.tmp.{}", std::process::id()));

    fs::write(&tmp_path, &body).await.map_err(Error::Io)?;
    // Best-effort fsync of the temp file before rename. `tokio::fs`
    // doesn't expose fsync directly; the rename below is a strong
    // enough barrier on every OS we ship for the typical
    // application-state-file case.
    fs::rename(&tmp_path, &final_path)
        .await
        .map_err(Error::Io)?;
    Ok(())
}

/// Clear the active-user pointer. Used by tests; production code never
/// calls this (sign-out keeps the pointer per the design).
pub async fn clear_active_user(data_dir: &Path) -> Result<()> {
    match fs::remove_file(state_path(data_dir)).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::Io(err)),
    }
}

/// Resolve `(db_path, user_id)` from on-disk state.
///
/// - If `state.json` exists → `(<data_dir>/<user_id>/<db_filename>, <user_id>)`.
/// - Else → `(<data_dir>/<db_filename>, SOLO_USER_ID)`.
///
/// `db_filename` is [`DB_FILENAME`] for desktop / CLI builds and
/// [`MOBILE_DB_FILENAME`] for Android.
pub async fn resolve_active_db(data_dir: &Path, db_filename: &str) -> Result<(PathBuf, String)> {
    match read_active_user(data_dir).await? {
        Some(user_id) => {
            let path = user_db_path(data_dir, &user_id, db_filename)?;
            Ok((path, user_id))
        }
        None => Ok((
            solo_db_path(data_dir, db_filename),
            SOLO_USER_ID.to_string(),
        )),
    }
}

/// One-time legacy migration on first authenticated sign-in.
///
/// See module docs for the invariant. Idempotent: subsequent calls
/// return `AlreadyMigrated` once the user's directory exists.
///
/// Order: rename first (atomic on a single FS), then row-rewrite. If
/// the rewrite fails after the rename, the destination is left in
/// place with SOLO_USER_ID-stamped rows; a later sync push would
/// surface that mismatch and the user can re-run the migration. The
/// safety-net path also runs the rewrite from `AlreadyMigrated`, so a
/// re-invocation closes the gap without manual intervention.
pub async fn migrate_solo_db_to_user(
    data_dir: &Path,
    user_id: &str,
    db_filename: &str,
) -> Result<SoloMigrationOutcome> {
    validate_user_id(user_id)?;

    let solo_path = solo_db_path(data_dir, db_filename);
    let user_path = user_db_path(data_dir, user_id, db_filename)?;
    let user_dir = user_path
        .parent()
        .expect("user_db_path always has a parent")
        .to_path_buf();

    let solo_exists = fs::try_exists(&solo_path).await.map_err(Error::Io)?;
    let user_exists = fs::try_exists(&user_path).await.map_err(Error::Io)?;

    match (solo_exists, user_exists) {
        (false, false) => Ok(SoloMigrationOutcome::NoSoloDb),

        (false, true) => {
            // Already-migrated; run the row rewrite as a safety net in
            // case a previous run died after rename but before UPDATE.
            // Idempotent: a fully-rewritten DB has no SOLO_USER_ID rows
            // left, so the UPDATEs find nothing to do.
            rewrite_user_id_columns(&user_path, user_id).await?;
            Ok(SoloMigrationOutcome::AlreadyMigrated)
        }

        (true, true) => {
            // The destination already holds something AND the legacy
            // DB is still on disk — we can't reconcile this safely. The
            // user's per-user DB likely belongs to them (we put it
            // there last time) and the legacy DB is foreign data. Per
            // CLAUDE.md: refuse silent fallback; raise.
            Err(Error::InvalidInput(format!(
                "cannot migrate {} into {}: both files exist. Resolve manually \
                 (the per-user DB is the canonical one; the legacy file is \
                 leftover offline-capture data that needs to be merged or \
                 archived).",
                solo_path.display(),
                user_path.display()
            )))
        }

        (true, false) => {
            // The normal first-signin path.
            fs::create_dir_all(&user_dir).await.map_err(Error::Io)?;
            fs::rename(&solo_path, &user_path)
                .await
                .map_err(Error::Io)?;
            rewrite_user_id_columns(&user_path, user_id).await?;
            Ok(SoloMigrationOutcome::Migrated)
        }
    }
}

/// Rewrite every `user_id` column in the moved DB to `user_id`.
///
/// The three places `user_id` appears in the schema after migration
/// v4 (see `db/migrations.rs`): `notes`, `pending_sync`, `sync_state`.
async fn rewrite_user_id_columns(db_path: &Path, user_id: &str) -> Result<()> {
    let database = Database::open(db_path).await?;
    let conn = database.connection();
    // Three UPDATEs in series — small enough that a transaction is
    // unnecessary, and keeping them atomic-per-table makes the
    // partial-failure story easier to reason about.
    conn.execute(
        "UPDATE notes SET user_id = ?1 WHERE user_id = ?2",
        libsql::params![user_id, SOLO_USER_ID],
    )
    .await?;
    conn.execute(
        "UPDATE pending_sync SET user_id = ?1 WHERE user_id = ?2",
        libsql::params![user_id, SOLO_USER_ID],
    )
    .await?;
    conn.execute(
        "UPDATE sync_state SET user_id = ?1 WHERE user_id = ?2",
        libsql::params![user_id, SOLO_USER_ID],
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::DatabaseService;
    use tempfile::TempDir;

    /// Canonical UUID-v7-shaped `user_id` for tests. Different from
    /// `SOLO_USER_ID` so migrations can prove they rewrite.
    const TEST_USER_A: &str = "01932aaa-0000-7000-8000-000000000001";
    const TEST_USER_B: &str = "01932bbb-0000-7000-8000-000000000002";

    fn data_dir() -> TempDir {
        TempDir::new().expect("tempdir")
    }

    #[test]
    fn solo_db_path_is_top_level_dirt_db() {
        let dir = std::path::PathBuf::from("/tmp/dirt");
        assert_eq!(solo_db_path(&dir, DB_FILENAME), dir.join("dirt.db"));
    }

    #[test]
    fn user_db_path_nests_under_user_id() {
        let dir = std::path::PathBuf::from("/tmp/dirt");
        let p = user_db_path(&dir, TEST_USER_A, DB_FILENAME).unwrap();
        assert_eq!(p, dir.join(TEST_USER_A).join("dirt.db"));
    }

    #[test]
    fn validate_user_id_accepts_uuid_v7() {
        assert!(validate_user_id(TEST_USER_A).is_ok());
        assert!(validate_user_id(SOLO_USER_ID).is_ok());
    }

    #[test]
    fn validate_user_id_rejects_empty() {
        assert!(validate_user_id("").is_err());
    }

    #[test]
    fn validate_user_id_rejects_path_separators() {
        assert!(validate_user_id("../etc").is_err());
        assert!(validate_user_id("a/b").is_err());
        assert!(validate_user_id("a\\b").is_err());
        assert!(validate_user_id("a\0b").is_err());
    }

    #[test]
    fn validate_user_id_rejects_dot_segments() {
        assert!(validate_user_id(".").is_err());
        assert!(validate_user_id("..").is_err());
    }

    #[test]
    fn validate_user_id_rejects_non_uuid() {
        assert!(validate_user_id("not-a-uuid").is_err());
        assert!(validate_user_id("abcdefgh").is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_active_user_returns_none_when_absent() {
        let dir = data_dir();
        let got = read_active_user(dir.path()).await.unwrap();
        assert_eq!(got, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_then_read_active_user_round_trips() {
        let dir = data_dir();
        write_active_user(dir.path(), TEST_USER_A).await.unwrap();
        let got = read_active_user(dir.path()).await.unwrap();
        assert_eq!(got, Some(TEST_USER_A.to_string()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_active_user_rejects_bad_user_id() {
        let dir = data_dir();
        assert!(write_active_user(dir.path(), "").await.is_err());
        assert!(write_active_user(dir.path(), "../escape").await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_active_user_overwrites_previous_value() {
        let dir = data_dir();
        write_active_user(dir.path(), TEST_USER_A).await.unwrap();
        write_active_user(dir.path(), TEST_USER_B).await.unwrap();
        let got = read_active_user(dir.path()).await.unwrap();
        assert_eq!(got, Some(TEST_USER_B.to_string()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_active_user_surfaces_corrupt_file_loudly() {
        let dir = data_dir();
        fs::write(state_path(dir.path()), b"not json")
            .await
            .unwrap();
        let err = read_active_user(dir.path()).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_active_user_rejects_bad_user_id_in_state_file() {
        let dir = data_dir();
        fs::write(state_path(dir.path()), br#"{"active_user_id":"../escape"}"#)
            .await
            .unwrap();
        let err = read_active_user(dir.path()).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_active_db_falls_back_to_solo_when_state_absent() {
        let dir = data_dir();
        let (path, user_id) = resolve_active_db(dir.path(), DB_FILENAME).await.unwrap();
        assert_eq!(path, solo_db_path(dir.path(), DB_FILENAME));
        assert_eq!(user_id, SOLO_USER_ID);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_active_db_returns_user_path_when_state_present() {
        let dir = data_dir();
        write_active_user(dir.path(), TEST_USER_A).await.unwrap();
        let (path, user_id) = resolve_active_db(dir.path(), DB_FILENAME).await.unwrap();
        assert_eq!(
            path,
            user_db_path(dir.path(), TEST_USER_A, DB_FILENAME).unwrap()
        );
        assert_eq!(user_id, TEST_USER_A);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrate_solo_db_no_solo_db_yields_no_op() {
        let dir = data_dir();
        let outcome = migrate_solo_db_to_user(dir.path(), TEST_USER_A, DB_FILENAME)
            .await
            .unwrap();
        assert_eq!(outcome, SoloMigrationOutcome::NoSoloDb);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrate_solo_db_moves_file_and_rewrites_user_id_columns() {
        let dir = data_dir();

        // Seed a legacy solo DB with content, a pending row, and a
        // sync_state row. The migration must rewrite all three.
        let solo_path = solo_db_path(dir.path(), DB_FILENAME);
        let db = DatabaseService::open_local_path(solo_path.clone())
            .await
            .unwrap();
        let note = db.create_note("legacy capture").await.unwrap();
        // Note creation already enqueues into pending_sync; sync_state
        // gets touched via write_sync_cursor.
        let cursor = crate::db::SyncCursor {
            sua: 42,
            id: note.id.to_string(),
        };
        db.write_sync_cursor(SOLO_USER_ID, &cursor).await.unwrap();
        drop(db);

        let outcome = migrate_solo_db_to_user(dir.path(), TEST_USER_A, DB_FILENAME)
            .await
            .unwrap();
        assert_eq!(outcome, SoloMigrationOutcome::Migrated);

        // Source gone, destination present.
        assert!(!fs::try_exists(&solo_path).await.unwrap());
        let user_path = user_db_path(dir.path(), TEST_USER_A, DB_FILENAME).unwrap();
        assert!(fs::try_exists(&user_path).await.unwrap());

        // Reopen and verify rows are rewritten.
        let moved = DatabaseService::open_local_path(user_path.clone())
            .await
            .unwrap();
        let notes = moved.list_notes(10, 0).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].user_id, TEST_USER_A);
        assert!(moved.is_pending(TEST_USER_A, &notes[0].id).await.unwrap());
        // Old SOLO key should be empty now.
        assert!(!moved.is_pending(SOLO_USER_ID, &notes[0].id).await.unwrap());
        // Sync cursor was on SOLO; should now read under the new key.
        let read_back = moved.read_sync_cursor(TEST_USER_A).await.unwrap();
        assert!(read_back.is_some());
        let solo_cursor = moved.read_sync_cursor(SOLO_USER_ID).await.unwrap();
        assert!(solo_cursor.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrate_solo_db_second_call_is_already_migrated() {
        let dir = data_dir();
        let solo_path = solo_db_path(dir.path(), DB_FILENAME);
        let db = DatabaseService::open_local_path(solo_path).await.unwrap();
        db.create_note("first").await.unwrap();
        drop(db);

        let first = migrate_solo_db_to_user(dir.path(), TEST_USER_A, DB_FILENAME)
            .await
            .unwrap();
        assert_eq!(first, SoloMigrationOutcome::Migrated);

        let second = migrate_solo_db_to_user(dir.path(), TEST_USER_A, DB_FILENAME)
            .await
            .unwrap();
        assert_eq!(second, SoloMigrationOutcome::AlreadyMigrated);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrate_solo_db_refuses_when_both_files_exist() {
        let dir = data_dir();
        // Create both: legacy + user dir. We'd hit this only after a
        // crashed half-migration that re-created the legacy file
        // somehow; refuse rather than overwrite.
        let solo_path = solo_db_path(dir.path(), DB_FILENAME);
        let db = DatabaseService::open_local_path(solo_path).await.unwrap();
        db.create_note("solo").await.unwrap();
        drop(db);

        let user_path = user_db_path(dir.path(), TEST_USER_A, DB_FILENAME).unwrap();
        fs::create_dir_all(user_path.parent().unwrap())
            .await
            .unwrap();
        let user_db = DatabaseService::open_local_path(user_path).await.unwrap();
        user_db.create_note("user").await.unwrap();
        drop(user_db);

        let err = migrate_solo_db_to_user(dir.path(), TEST_USER_A, DB_FILENAME)
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::InvalidInput(_)),
            "expected refusal, got {err:?}"
        );
    }

    /// Regression: if a previous migration succeeded at rename but
    /// crashed before the row rewrite, the second call must still
    /// complete the rewrite so the DB ends up with the right `user_id`
    /// columns. We simulate by opening the user DB directly with
    /// `SOLO_USER_ID` rows and then calling the migration (which sees
    /// `AlreadyMigrated` but runs the rewrite anyway).
    #[tokio::test(flavor = "current_thread")]
    async fn migrate_solo_db_repairs_half_migrated_state() {
        let dir = data_dir();
        let user_path = user_db_path(dir.path(), TEST_USER_A, DB_FILENAME).unwrap();
        fs::create_dir_all(user_path.parent().unwrap())
            .await
            .unwrap();
        // Stage SOLO-stamped rows in what would be the user's DB:
        let db = DatabaseService::open_local_path(user_path.clone())
            .await
            .unwrap();
        let note = db.create_note("half-migrated note").await.unwrap();
        assert_eq!(note.user_id, SOLO_USER_ID);
        drop(db);

        let outcome = migrate_solo_db_to_user(dir.path(), TEST_USER_A, DB_FILENAME)
            .await
            .unwrap();
        assert_eq!(outcome, SoloMigrationOutcome::AlreadyMigrated);

        let repaired = DatabaseService::open_local_path(user_path).await.unwrap();
        let notes = repaired.list_notes(10, 0).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(
            notes[0].user_id, TEST_USER_A,
            "AlreadyMigrated must repair stale SOLO_USER_ID rows"
        );
    }
}
