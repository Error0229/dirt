//! What the store keeps for sync: which notes are unsent, what the server
//! last said about each note, how far pulling has got, and which notes could
//! not be synced and why.
//!
//! A note is **unsent** while its revision differs from the revision last
//! pushed. Only a local edit raises the revision. Merging a document that
//! came from the server does not, so pulling never makes a note look unsent,
//! and an edit made while a push is in flight is never mistaken for sent.

use rusqlite::{params, Connection, OptionalExtension};

use super::notes::{load_for_update, save, Origin};
use super::schema::{client_id, meta_get, meta_set, rotate_client_id, write_transaction};
use crate::models::{Note, NoteId};
use crate::notes::NoteDoc;
use crate::{Error, Result};

/// A note waiting to be pushed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsentNote {
    pub id: NoteId,
    /// The note's whole document, as it would be uploaded once encrypted.
    pub document: Vec<u8>,
    /// The revision this document is. Passing it back to
    /// [`mark_pushed`](crate::Store::mark_note_pushed) is how an edit made
    /// during the push stays unsent.
    pub revision: i64,
    /// The change number of the server's version this document builds on,
    /// or zero if the server has never had the note.
    pub base_change: i64,
    /// Where this note sits in the store's order. Passing the last note's
    /// position as `after` fetches the notes that follow it.
    pub position: i64,
}

/// How far sync has got with the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncProgress {
    /// The change number up to which everything has been pulled.
    pub cursor: i64,
    /// The account epoch the cursor and every change number belong to, or
    /// zero before the first sync.
    pub epoch: i32,
}

/// A note that could not be synced, and why. It is not pushed while the
/// problem stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteProblem {
    pub note_id: NoteId,
    pub reason: String,
    pub kind: ProblemKind,
    /// The note's text on this device, to name it by. `None` for a note
    /// this device does not hold: a blob that could not be read may belong
    /// to a note it has never had.
    pub text: Option<String>,
}

/// What ends a note's sync problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    /// The server's copy could not be read. The note is held until a copy
    /// arrives that can be, so that it is never pushed over unread.
    Held,
    /// The server refused this device's copy, for example as too large.
    /// Editing the note gives it another try.
    RetryAfterEdit,
}

fn to_sql_count(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// Unsent notes positioned after `after`, in the store's order, up to
/// `max_notes` and for as long as their documents fit in `max_bytes`
/// together. At least one is returned if any remains.
pub(super) fn unsent(
    connection: &Connection,
    after: i64,
    max_notes: usize,
    max_bytes: usize,
) -> Result<Vec<UnsentNote>> {
    let mut statement = connection.prepare(
        "SELECT id, doc, revision, server_change, rowid FROM notes \
          WHERE (pushed_revision IS NULL OR pushed_revision <> revision) \
            AND id NOT IN (SELECT note_id FROM sync_problems) \
            AND rowid > ?1 \
          ORDER BY rowid LIMIT ?2",
    )?;
    let rows = statement.query_map([after, to_sql_count(max_notes)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    let mut notes = Vec::new();
    let mut bytes = 0usize;
    for row in rows {
        let (id, document, revision, base_change, position) = row?;
        bytes += document.len();
        if bytes > max_bytes && !notes.is_empty() {
            break;
        }
        let id = id
            .parse()
            .map_err(|_| Error::Database(format!("the store holds an invalid note ID: {id}")))?;
        notes.push(UnsentNote {
            id,
            document,
            revision,
            base_change,
            position,
        });
    }
    Ok(notes)
}

pub(super) fn unsent_count(connection: &Connection) -> Result<usize> {
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM notes \
          WHERE (pushed_revision IS NULL OR pushed_revision <> revision) \
            AND id NOT IN (SELECT note_id FROM sync_problems)",
        [],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or_default())
}

/// How many notes hold changes the server has not accepted, including the
/// notes that are held back by a sync problem. This is what would be lost
/// if the store were deleted now.
pub(super) fn unsynced_count(connection: &Connection) -> Result<usize> {
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM notes WHERE pushed_revision IS NULL OR pushed_revision <> revision",
        [],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or_default())
}

/// When a sync cycle last completed, as Unix milliseconds.
pub(super) fn last_synced(connection: &Connection) -> Result<Option<i64>> {
    Ok(meta_get(connection, "last_synced_ms")?.and_then(|value| value.parse().ok()))
}

pub(super) fn set_last_synced(connection: &mut Connection, at_ms: i64) -> Result<()> {
    let tx = write_transaction(connection)?;
    meta_set(&tx, "last_synced_ms", &at_ms.to_string())?;
    tx.commit()?;
    Ok(())
}

/// Records that the server accepted `revision` of a note as change number
/// `change`. If the note has been edited since, it stays unsent: its
/// revision is by then higher than the one recorded here.
///
/// A change number that is not newer than the one already recorded is
/// ignored. Two processes share a store, and the one whose push the server
/// took first may be the one that records it last.
pub(super) fn mark_pushed(
    connection: &mut Connection,
    id: &NoteId,
    revision: i64,
    change: i64,
) -> Result<()> {
    let tx = write_transaction(connection)?;
    tx.execute(
        "UPDATE notes SET pushed_revision = ?2, server_change = ?3 \n          WHERE id = ?1 AND server_change < ?3",
        params![id.as_str(), revision, change],
    )?;
    tx.commit()?;
    Ok(())
}

/// Merges a document that came from the server, as change number `change`,
/// into the local copy of the note, creating the note if it is new here.
/// Returns the note, or `None` if the merged document says it is deleted.
///
/// A change number lower than one already seen for the note is refused: the
/// server is replaying an old version.
///
/// If the incoming document holds more of this install's own edits than the
/// local copy does, the install has lost state, for example by restoring an
/// old backup. It then takes a new CRDT client ID before merging, because
/// writing more under the old one would reuse edit numbers.
pub(super) fn apply_remote(
    connection: &mut Connection,
    id: &NoteId,
    incoming: &[u8],
    change: i64,
) -> Result<Option<Note>> {
    let tx = write_transaction(connection)?;
    let existing = load_for_update(&tx, id)?;
    if let Some((_, _, seen)) = &existing {
        if change < *seen {
            return Err(Error::Replay {
                note: id.to_string(),
                seen: *seen,
                got: change,
            });
        }
    }

    let mut me = client_id(&tx)?;
    let mine = existing
        .as_ref()
        .map(|(_, document, _)| document.as_slice());
    let local = match mine {
        Some(document) => NoteDoc::from_bytes(me, document)?,
        None => NoteDoc::new(me),
    };
    let doc = if local.incoming_knows_more_of_me(incoming)? {
        me = rotate_client_id(&tx)?;
        tracing::warn!("this install had lost edits it once made; it now writes as client {me}");
        match mine {
            Some(document) => NoteDoc::from_bytes(me, document)?,
            None => NoteDoc::new(me),
        }
    } else {
        local
    };
    doc.merge(incoming)?;

    let row = existing.as_ref().map(|(row, _, _)| *row);
    let note = save(&tx, id, &doc, row, Origin::Remote)?;
    tx.execute(
        "UPDATE notes SET server_change = ?2 WHERE id = ?1",
        params![id.as_str(), change],
    )?;
    tx.execute(
        "DELETE FROM sync_problems WHERE note_id = ?1",
        [id.as_str()],
    )?;
    tx.commit()?;
    Ok(note)
}

/// Makes a note unsent without changing it. What the server is known to
/// hold for it stays as it is, so the push replaces exactly that.
pub(super) fn resend(connection: &mut Connection, id: &NoteId) -> Result<bool> {
    let tx = write_transaction(connection)?;
    let marked = tx.execute(
        "UPDATE notes SET pushed_revision = NULL \
          WHERE id = ?1 AND id NOT IN (SELECT note_id FROM sync_problems)",
        [id.as_str()],
    )?;
    tx.commit()?;
    Ok(marked > 0)
}

/// A deleted note that this device may ask the server to purge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeCandidate {
    pub id: NoteId,
    /// The local revision counter when the note was listed.
    pub revision: i64,
    /// The change number of the version the server holds, which is the
    /// version this device holds.
    pub server_change: i64,
}

/// What became of this device's copy of a note that the server has purged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeApplied {
    /// This device does not hold the note.
    NotHeld,
    /// The copy was removed. Everything in it was in the version that was
    /// purged, or the note is deleted here too.
    Dropped,
    /// The copy held an edit that the purge did not cover and the note is
    /// not deleted here. Its text was kept, as this new note.
    KeptAsNew(NoteId),
}

/// Deleted notes that this device has seen deleted since before
/// `deleted_before_ms`, and that the server holds exactly as this device
/// does. Up to `max`, in the store's order.
///
/// A note with a change not yet sent is left out: the server would refuse
/// to purge a version it does not hold. So is a note with a sync problem.
pub(super) fn purge_candidates(
    connection: &Connection,
    deleted_before_ms: i64,
    max: usize,
) -> Result<Vec<PurgeCandidate>> {
    let mut statement = connection.prepare(
        "SELECT id, revision, server_change FROM notes \
          WHERE deleted = 1 AND deleted_at <= ?1 \
            AND pushed_revision = revision AND server_change > 0 \
            AND id NOT IN (SELECT note_id FROM sync_problems) \
          ORDER BY rowid LIMIT ?2",
    )?;
    let rows = statement.query_map(params![deleted_before_ms, to_sql_count(max)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut candidates = Vec::new();
    for row in rows {
        let (id, revision, server_change) = row?;
        let id = id
            .parse()
            .map_err(|_| Error::Database(format!("the store holds an invalid note ID: {id}")))?;
        candidates.push(PurgeCandidate {
            id,
            revision,
            server_change,
        });
    }
    Ok(candidates)
}

fn remove_row(tx: &rusqlite::Transaction<'_>, row: i64, id: &NoteId) -> Result<()> {
    tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", [row])?;
    tx.execute(
        "DELETE FROM sync_problems WHERE note_id = ?1",
        [id.as_str()],
    )?;
    // The note's tag rows go with it, through the foreign key.
    tx.execute("DELETE FROM notes WHERE rowid = ?1", [row])?;
    Ok(())
}

/// Removes a note that the server purged at this device's request, if the
/// note is still deleted and has not changed since it was listed. Returns
/// whether it was removed.
///
/// A note that changed in the meantime stays. The purge marker reaches
/// this device at its next pull, and [`apply_purge`] decides then.
pub(super) fn drop_purged(connection: &mut Connection, id: &NoteId, revision: i64) -> Result<bool> {
    let tx = write_transaction(connection)?;
    let row: Option<i64> = tx
        .query_row(
            "SELECT rowid FROM notes \
              WHERE id = ?1 AND deleted = 1 AND revision = ?2 AND pushed_revision = ?2",
            params![id.as_str(), revision],
            |row| row.get(0),
        )
        .optional()?;
    let Some(row) = row else {
        return Ok(false);
    };
    remove_row(&tx, row, id)?;
    tx.commit()?;
    Ok(true)
}

/// Takes the news that the server has purged a note.
///
/// The device that asked for the purge had seen the note deleted for a
/// long time, and the server purged exactly the version that device held.
/// So whatever this device had sent of the note was in that version and
/// was covered by the delete, and its copy can go.
///
/// The one thing the purge cannot have covered is a change this device has
/// not sent. If the note is deleted here as well, that change is the
/// delete, and nothing is lost. If it is not, the person wrote something
/// here that no other device has seen, and it is never thrown away: the
/// text is kept as a new note, which syncs like any other.
pub(super) fn apply_purge(connection: &mut Connection, id: &NoteId) -> Result<PurgeApplied> {
    let tx = write_transaction(connection)?;
    let held: Option<(i64, Vec<u8>, bool, bool)> = tx
        .query_row(
            "SELECT rowid, doc, deleted, \
                    pushed_revision IS NULL OR pushed_revision <> revision \
               FROM notes WHERE id = ?1",
            [id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((row, document, deleted, unsent)) = held else {
        // A note this device could never read is listed as a problem
        // without being held: it joined after a change of the key, say.
        // The note is gone now, and the problem with it. Left behind, it
        // would be reported for ever, with no later version to cure it.
        tx.execute(
            "DELETE FROM sync_problems WHERE note_id = ?1",
            [id.as_str()],
        )?;
        tx.commit()?;
        return Ok(PurgeApplied::NotHeld);
    };
    let applied = if unsent && !deleted {
        let me = client_id(&tx)?;
        let old = NoteDoc::from_bytes(me, &document)?;
        let kept = NoteId::new();
        let fresh = NoteDoc::new(me);
        // The note keeps the time it was last edited, so that it stays
        // where it was in the list.
        let edited_at = old.edited_at_ms().unwrap_or_else(crate::util::now_ms);
        fresh.set_text(&old.text(), edited_at);
        if old.text().is_empty() {
            // As for any new note with no text: this gives it an edit time.
            fresh.restore(edited_at);
        }
        save(&tx, &kept, &fresh, None, Origin::Local)?;
        PurgeApplied::KeptAsNew(kept)
    } else {
        PurgeApplied::Dropped
    };
    remove_row(&tx, row, id)?;
    tx.commit()?;
    Ok(applied)
}

pub(super) fn progress(connection: &Connection) -> Result<SyncProgress> {
    let read = |key: &str| -> Result<Option<i64>> {
        Ok(meta_get(connection, key)?.and_then(|value| value.parse().ok()))
    };
    Ok(SyncProgress {
        cursor: read("pull_cursor")?.unwrap_or(0),
        epoch: read("account_epoch")?
            .and_then(|epoch| i32::try_from(epoch).ok())
            .unwrap_or(0),
    })
}

pub(super) fn set_cursor(connection: &mut Connection, cursor: i64) -> Result<()> {
    let tx = write_transaction(connection)?;
    meta_set(&tx, "pull_cursor", &cursor.to_string())?;
    tx.commit()?;
    Ok(())
}

/// Starts over for a new account epoch: the cursor returns to zero, every
/// change number seen is forgotten, and every note becomes unsent. The next
/// sync then pulls everything, merges it, and pushes what the server lacks.
///
/// The server raises the epoch when the account is reset or restored from a
/// backup. Its change numbers then start again below the ones seen before,
/// which the replay check would otherwise refuse for ever.
pub(super) fn begin_epoch(connection: &mut Connection, epoch: i32) -> Result<()> {
    let tx = write_transaction(connection)?;
    meta_set(&tx, "account_epoch", &epoch.to_string())?;
    meta_set(&tx, "pull_cursor", "0")?;
    tx.execute(
        "UPDATE notes SET server_change = 0, pushed_revision = NULL",
        [],
    )?;
    tx.execute("DELETE FROM sync_problems", [])?;
    tx.commit()?;
    Ok(())
}

pub(super) fn set_problem(
    connection: &mut Connection,
    id: &NoteId,
    problem: Option<(&str, ProblemKind)>,
) -> Result<()> {
    let tx = write_transaction(connection)?;
    match problem {
        Some((reason, kind)) => tx.execute(
            "INSERT OR REPLACE INTO sync_problems (note_id, reason, retry_after_edit) \
             VALUES (?1, ?2, ?3)",
            params![id.as_str(), reason, kind == ProblemKind::RetryAfterEdit],
        )?,
        None => tx.execute(
            "DELETE FROM sync_problems WHERE note_id = ?1",
            [id.as_str()],
        )?,
    };
    tx.commit()?;
    Ok(())
}

pub(super) fn problems(connection: &Connection) -> Result<Vec<NoteProblem>> {
    let mut statement = connection.prepare(
        "SELECT p.note_id, p.reason, p.retry_after_edit, n.text \
           FROM sync_problems p LEFT JOIN notes n ON n.id = p.note_id \
          ORDER BY p.note_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, bool>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut problems = Vec::new();
    for row in rows {
        let (id, reason, retry_after_edit, text) = row?;
        if let Ok(note_id) = id.parse() {
            let kind = if retry_after_edit {
                ProblemKind::RetryAfterEdit
            } else {
                ProblemKind::Held
            };
            problems.push(NoteProblem {
                note_id,
                reason,
                kind,
                text,
            });
        }
    }
    Ok(problems)
}

/// Forgets the server's version of a note, so that the next push offers
/// it as new. Used when the server reports that it has no such note.
pub(super) fn forget_server_version(connection: &mut Connection, id: &NoteId) -> Result<()> {
    let tx = write_transaction(connection)?;
    tx.execute(
        "UPDATE notes SET server_change = 0, pushed_revision = NULL WHERE id = ?1",
        [id.as_str()],
    )?;
    tx.commit()?;
    Ok(())
}

/// The last change number seen for a note, or `None` if the note is not in
/// the store.
pub(super) fn server_change(connection: &Connection, id: &NoteId) -> Result<Option<i64>> {
    Ok(connection
        .query_row(
            "SELECT server_change FROM notes WHERE id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .optional()?)
}
