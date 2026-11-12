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
//!
//! A fourth came the same way, from a write that failed once on a loaded
//! machine with the file busy: a write waits half a minute for its turn,
//! and looks often. SQLite's own waiting looks ten times a second and
//! gives no process a turn. A process that is short of processor time is
//! mostly paused while it holds the file, and when it runs again it
//! finishes one write and begins the next in the same breath, so another
//! process that looks only now and then can find the file taken every
//! time for many seconds. Forty copies of the two-process test at once
//! showed it: with a wait of five seconds more than half of them failed.
//! A thought that could not be saved is the one failure this app must not
//! have, so a write waits as long as a person plausibly would.

use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior};

use crate::Result;

/// How long a connection waits for another process's write to finish.
pub(super) const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a connection pauses between two looks at a busy file. Short,
/// so that the moment the file is free is not missed.
const BUSY_PAUSE: Duration = Duration::from_millis(5);

/// How long opening a store keeps trying while the file is busy.
const OPEN_PATIENCE: Duration = Duration::from_secs(60);

/// What a connection does when it finds the file busy: pause and say to
/// look again, until it has paused for [`BUSY_TIMEOUT`] in all. `looks` is
/// how many times it has found the file busy so far in this one wait.
fn wait_for_the_file(looks: i32) -> bool {
    let waited = BUSY_PAUSE.saturating_mul(u32::try_from(looks).unwrap_or(u32::MAX));
    if waited >= BUSY_TIMEOUT {
        return false;
    }
    std::thread::sleep(BUSY_PAUSE);
    true
}

/// The version of the tables below, kept in SQLite's `user_version`.
const SCHEMA_VERSION: i64 = 5;

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
    connection.busy_handler(Some(wait_for_the_file))?;

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

/// Version 4: which files each note names, derived from its document as the
/// tags are. A deleted note keeps its rows: its document still names the
/// files, and they are wanted for as long as the note can come back.
const SCHEMA_V4: &str = "
CREATE TABLE note_files (
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    file_id TEXT NOT NULL,
    PRIMARY KEY (note_id, file_id)
) WITHOUT ROWID;
CREATE INDEX note_files_by_file ON note_files (file_id);
";

/// Version 5: what this device has told the server about each file a note
/// names, and what it still has to. The server cannot read a note, so it
/// learns which notes refer to which files from the devices.
///
/// A row of `file_links` is a file and a note that named it. `confirmed`
/// is set once the server has taken the reference. `unnamed_at` is when
/// this device first saw the note without the file; after a wait it tells
/// the server so, as it asks for a deleted note to be purged, and the row
/// goes. `problem` is why the file could not be sent, if it could not. A
/// note's rows go with the note.
///
/// A row of `shelf_spare` is a file on the shelf that nothing needed when
/// the device last looked, and since when. Such a file is removed only
/// after it has stayed unneeded for a while: a file is put on the shelf a
/// moment before the note that names it is written, and another program on
/// this device may be doing just that.
const SCHEMA_V5: &str = "
CREATE TABLE file_links (
    file_id    TEXT NOT NULL,
    note_id    TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    confirmed  INTEGER NOT NULL DEFAULT 0,
    unnamed_at INTEGER,
    problem    TEXT,
    PRIMARY KEY (file_id, note_id)
);
CREATE INDEX file_links_by_note ON file_links (note_id);
INSERT INTO file_links (file_id, note_id) SELECT file_id, note_id FROM note_files;
CREATE TABLE shelf_spare (
    file_id TEXT PRIMARY KEY,
    since   INTEGER NOT NULL
) WITHOUT ROWID;
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
    if version < 4 {
        tx.execute_batch(SCHEMA_V4)?;
    }
    if version < 5 {
        tx.execute_batch(SCHEMA_V5)?;
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

    /// A database written before files were sent anywhere. Each file a
    /// note names in it has a reference the server has yet to be told of.
    #[test]
    fn a_database_from_before_files_were_sent_has_every_reference_to_confirm() {
        let (_folder, path) = fresh();
        {
            let connection = open_connection(&path).unwrap();
            for schema in [SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4] {
                connection.execute_batch(schema).unwrap();
            }
            connection.pragma_update(None, "user_version", 4).unwrap();
            connection
                .execute_batch(
                    "INSERT INTO notes (id, doc, text, edited_at, deleted, revision) \
                     VALUES ('with', x'00', 'with', 1, 0, 1), ('without', x'00', 'without', 1, 0, 1);
                     INSERT INTO note_files (note_id, file_id) VALUES ('with', 'f1'), ('with', 'f2');",
                )
                .unwrap();
        }

        let mut connection = open_connection(&path).unwrap();
        set_up(&mut connection).unwrap();

        let mut statement = connection
            .prepare(
                "SELECT file_id, note_id, confirmed, unnamed_at IS NULL, problem IS NULL \
                   FROM file_links ORDER BY file_id",
            )
            .unwrap();
        let links: Vec<(String, String, bool, bool, bool)> = statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            links,
            [
                ("f1".to_owned(), "with".to_owned(), false, true, true),
                ("f2".to_owned(), "with".to_owned(), false, true, true),
            ]
        );
        // A note's references go with the note.
        connection
            .execute("DELETE FROM notes WHERE id = 'with'", [])
            .unwrap();
        let left: i64 = connection
            .query_row("SELECT count(*) FROM file_links", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, 0);
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
