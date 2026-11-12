//! Opening the database file and setting up its tables.
//!
//! The desktop app and the CLI open the same file from two processes. Two
//! rules make that safe, and trial 2 of the redesign showed each is needed
//! (`docs/trials/2026-11-11-trial-2-rusqlite-android-and-two-processes.md`):
//!
//! 1. Opening sets the busy timeout first, then retries the switch to
//!    write-ahead-log mode. The busy timeout does not cover that switch.
//! 2. Every write transaction starts with `BEGIN IMMEDIATE`, including the
//!    one that sets up the tables.
//!
//! A third rule came from a test that failed once on a loaded machine: a
//! store that finds the file busy while it opens tries again, for longer
//! than one busy timeout. Several things can make an open find the file
//! busy that waiting inside SQLite does not cover or cannot outlast: other
//! processes writing without a pause, and the moment in which the last
//! connection of another process closes and SQLite has the file to itself.
//! A program that cannot open its notes cannot do anything, so opening is
//! given more patience than a single write.

use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior};

use crate::Result;

/// How long a connection waits for another process's write to finish.
pub(super) const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// How long opening a store keeps trying while the file is busy.
const OPEN_PATIENCE: Duration = Duration::from_secs(20);

/// The version of the tables below, kept in SQLite's `user_version`.
const SCHEMA_VERSION: i64 = 3;

const SCHEMA_V1: &str = "
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE notes (
    rowid           INTEGER PRIMARY KEY,
    id              TEXT NOT NULL UNIQUE,
    doc             BLOB NOT NULL,
    text            TEXT NOT NULL,
    edited_at       INTEGER NOT NULL,
    deleted         INTEGER NOT NULL,
    revision        INTEGER NOT NULL,
    pushed_revision INTEGER,
    server_change   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX notes_by_edit ON notes (deleted, edited_at DESC);

CREATE TABLE note_tags (
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    PRIMARY KEY (note_id, tag)
) WITHOUT ROWID;
CREATE INDEX note_tags_by_tag ON note_tags (tag);

CREATE VIRTUAL TABLE notes_fts USING fts5(text);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
";

/// Version 2: notes that could not be synced, and why. A note listed here
/// is not pushed. The note need not exist locally: a blob that could not
/// be read may belong to a note this device has never held.
const SCHEMA_V2: &str = "
CREATE TABLE sync_problems (
    note_id          TEXT PRIMARY KEY,
    reason           TEXT NOT NULL,
    retry_after_edit INTEGER NOT NULL
) WITHOUT ROWID;
";

fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// Runs one step of opening a store, and runs it again for as long as it
/// fails because the file is busy, up to [`OPEN_PATIENCE`]. The step must be
/// safe to repeat.
pub(super) fn retry_while_busy<T>(mut step: impl FnMut() -> Result<T>) -> Result<T> {
    let started = Instant::now();
    loop {
        match step() {
            Err(crate::Error::Sqlite(error))
                if is_busy(&error) && started.elapsed() < OPEN_PATIENCE =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            outcome => return outcome,
        }
    }
}

/// Opens one connection the way every Dirt process must.
pub(super) fn open_connection(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;

    // When two processes switch a fresh file at the same moment, SQLite fails
    // one of them at once instead of waiting. The mode is stored in the file,
    // so the retry finds the switch already made.
    let started = Instant::now();
    loop {
        match connection.pragma_update(None, "journal_mode", "WAL") {
            Ok(()) => break,
            Err(error) if is_busy(&error) && started.elapsed() < BUSY_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
    // FULL: a saved note survives power loss, at the cost of waiting for the
    // disk on each commit.
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    Ok(connection)
}

/// Opens a connection that may only read.
pub(super) fn open_reader(path: &Path) -> Result<Connection> {
    let connection = open_connection(path)?;
    connection.pragma_update(None, "query_only", "ON")?;
    Ok(connection)
}

/// Starts a write transaction. Every write in the store goes through here.
pub(super) fn write_transaction(connection: &mut Connection) -> Result<Transaction<'_>> {
    Ok(connection.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

/// Creates the tables unless another process already has, gives this install
/// its CRDT client ID, and returns the database's own identifier.
///
/// The identifier is made once and kept in the file. Two connections hold
/// the same database exactly when they read the same identifier, whatever
/// path each was opened through.
///
/// Version 3: when this device first saw a note as deleted, in milliseconds,
/// and nothing for a note that is not deleted. A device asks the server to
/// purge a note only when it has itself seen it deleted for long enough, so
/// no two devices' clocks are ever compared. Notes already deleted when the
/// column is added count as deleted from that moment.
const SCHEMA_V3: &str = "
ALTER TABLE notes ADD COLUMN deleted_at INTEGER;
UPDATE notes SET deleted_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000 WHERE deleted = 1;
";

/// The version is read inside the write transaction, so two processes that
/// start together cannot both run the set-up.
pub(super) fn set_up(connection: &mut Connection) -> Result<String> {
    let tx = write_transaction(connection)?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(crate::Error::Database(format!(
            "this database was written by a newer version of Dirt \
             (schema {version}, this build understands {SCHEMA_VERSION})"
        )));
    }
    if version < 1 {
        tx.execute_batch(SCHEMA_V1)?;
    }
    if version < 2 {
        tx.execute_batch(SCHEMA_V2)?;
    }
    if version < 3 {
        tx.execute_batch(SCHEMA_V3)?;
    }
    if version < SCHEMA_VERSION {
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    if read_client_id(&tx)?.is_none() {
        write_client_id(&tx, new_client_id())?;
    }
    let existing: Option<String> = tx
        .query_row("SELECT value FROM meta WHERE key = 'store_id'", [], |row| {
            row.get(0)
        })
        .optional()?;
    let store_id = if let Some(id) = existing {
        id
    } else {
        let id = uuid::Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('store_id', ?1)",
            [&id],
        )?;
        id
    };
    tx.commit()?;
    Ok(store_id)
}

/// The CRDT client ID every edit made through this file is written under.
///
/// It lives in the database so that it is lost together with the data it
/// describes: a fresh database is a fresh device as far as the CRDT is
/// concerned.
pub(super) fn client_id(tx: &Transaction<'_>) -> Result<u64> {
    read_client_id(tx)?
        .ok_or_else(|| crate::Error::Database("the store has no client ID".to_owned()))
}

fn read_client_id(tx: &Transaction<'_>) -> Result<Option<u64>> {
    let text: Option<String> = tx
        .query_row(
            "SELECT value FROM meta WHERE key = 'client_id'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(text.and_then(|value| value.parse().ok()))
}

fn write_client_id(tx: &Transaction<'_>, id: u64) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('client_id', ?1)",
        [id.to_string()],
    )?;
    Ok(())
}

/// Gives this install a new CRDT client ID and returns it. Used when the
/// install finds it has lost edits it once made: writing more under the
/// old ID would reuse edit numbers.
pub(super) fn rotate_client_id(tx: &Transaction<'_>) -> Result<u64> {
    let id = new_client_id();
    write_client_id(tx, id)?;
    Ok(id)
}

pub(super) fn meta_get(connection: &Connection, key: &str) -> Result<Option<String>> {
    Ok(connection
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

pub(super) fn meta_set(tx: &Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        [key, value],
    )?;
    Ok(())
}

/// A random client ID below 2^53, the largest value yrs accepts.
fn new_client_id() -> u64 {
    const MASK: u128 = (1 << 53) - 1;
    // The low bits of a version 7 UUID are random.
    let random = uuid::Uuid::now_v7().as_u128() & MASK;
    // A zero ID is legal but looks like "unset" in logs; avoid it.
    u64::try_from(random).unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (tempfile::TempDir, std::path::PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("notes.db");
        (folder, path)
    }

    #[test]
    fn a_new_file_gets_the_tables_and_a_client_id() {
        let (_folder, path) = fresh();
        let mut connection = open_connection(&path).unwrap();
        set_up(&mut connection).unwrap();

        let mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let tx = write_transaction(&mut connection).unwrap();
        let id = client_id(&tx).unwrap();
        assert!(id > 0 && id < (1 << 53));
    }

    #[test]
    fn setting_up_twice_keeps_the_client_id() {
        let (_folder, path) = fresh();
        let mut first = open_connection(&path).unwrap();
        set_up(&mut first).unwrap();
        let before = client_id(&write_transaction(&mut first).unwrap()).unwrap();
        drop(first);

        let mut second = open_connection(&path).unwrap();
        set_up(&mut second).unwrap();
        let after = client_id(&write_transaction(&mut second).unwrap()).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn two_new_files_get_different_client_ids() {
        let (_a, path_a) = fresh();
        let (_b, path_b) = fresh();
        let mut a = open_connection(&path_a).unwrap();
        let mut b = open_connection(&path_b).unwrap();
        set_up(&mut a).unwrap();
        set_up(&mut b).unwrap();
        let id_a = client_id(&write_transaction(&mut a).unwrap()).unwrap();
        let id_b = client_id(&write_transaction(&mut b).unwrap()).unwrap();
        assert_ne!(id_a, id_b);
    }

    /// A database written before notes recorded when they were deleted.
    /// Its deleted notes count as deleted from the moment it is upgraded:
    /// this device cannot know how long ago they were, and waiting the
    /// whole time again loses nothing.
    #[test]
    fn a_database_from_before_purging_counts_its_deleted_notes_from_now() {
        let (_folder, path) = fresh();
        {
            let connection = open_connection(&path).unwrap();
            connection.execute_batch(SCHEMA_V1).unwrap();
            connection.execute_batch(SCHEMA_V2).unwrap();
            connection.pragma_update(None, "user_version", 2).unwrap();
            connection
                .execute_batch(
                    "INSERT INTO notes (id, doc, text, edited_at, deleted, revision) \
                     VALUES ('live', x'00', 'live', 1, 0, 1), ('gone', x'00', '', 1, 1, 1)",
                )
                .unwrap();
        }
        let before = crate::util::now_ms() - 2_000;

        let mut connection = open_connection(&path).unwrap();
        set_up(&mut connection).unwrap();

        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let deleted_at = |id: &str| -> Option<i64> {
            connection
                .query_row("SELECT deleted_at FROM notes WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .unwrap()
        };
        assert_eq!(deleted_at("live"), None);
        let gone = deleted_at("gone").expect("a deleted note has a time");
        assert!(
            gone >= before && gone <= crate::util::now_ms() + 2_000,
            "{gone}"
        );
    }

    #[test]
    fn a_file_from_a_newer_version_is_refused() {
        let (_folder, path) = fresh();
        let mut connection = open_connection(&path).unwrap();
        set_up(&mut connection).unwrap();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        let error = set_up(&mut connection).unwrap_err();
        assert!(error.to_string().contains("newer version"), "{error}");
    }

    #[test]
    fn a_reader_cannot_write() {
        let (_folder, path) = fresh();
        let mut writer = open_connection(&path).unwrap();
        set_up(&mut writer).unwrap();
        let reader = open_reader(&path).unwrap();
        let result = reader.execute("INSERT INTO settings (key, value) VALUES ('a', 'b')", []);
        assert!(result.is_err());
    }
}
