//! What the store keeps about files for sync.
//!
//! The server cannot read a note, so it does not know which files a note
//! names. Devices tell it: they **confirm** that a note refers to a file,
//! and later **release** the reference. The table `file_links` holds what
//! this device has told the server and what it still has to.
//!
//! The rows follow the notes. Writing a note adds a row for each file it
//! names for the first time, and dates the row of a file it has stopped
//! naming (`notes::save`). Removing a note removes its rows.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::schema::write_transaction;
use crate::files::shelf::Shelf;
use crate::files::FilePart;
use crate::models::NoteId;
use crate::notes::NoteDoc;
use crate::{Error, Result};

/// A file and a note that names it, or did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileLink {
    pub file_id: Uuid,
    pub note_id: NoteId,
    /// Where the reference sits in the store's order. Passing the last
    /// one's position as `after` fetches those that follow it.
    pub position: i64,
}

/// A file that could not be sent for a note, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileProblem {
    pub file_id: Uuid,
    pub note_id: NoteId,
    pub reason: String,
}

fn to_sql_count(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

fn file_id(text: &str) -> Result<Uuid> {
    text.parse()
        .map_err(|_| Error::Database(format!("the store holds an invalid file ID: {text}")))
}

fn note_id(text: &str) -> Result<NoteId> {
    text.parse()
        .map_err(|_| Error::Database(format!("the store holds an invalid note ID: {text}")))
}

fn links(connection: &Connection, sql: &str, first: i64, max: usize) -> Result<Vec<FileLink>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params![first, to_sql_count(max)], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut links = Vec::new();
    for row in rows {
        let (position, file, note) = row?;
        links.push(FileLink {
            file_id: file_id(&file)?,
            note_id: note_id(&note)?,
            position,
        });
    }
    Ok(links)
}

/// One file that a note's document names, with its key.
pub(super) fn part(connection: &Connection, note: &NoteId, file: Uuid) -> Result<Option<FilePart>> {
    let document: Option<Vec<u8>> = connection
        .query_row(
            "SELECT doc FROM notes WHERE id = ?1",
            [note.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(document) = document else {
        return Ok(None);
    };
    // Reading needs no client ID of its own: nothing is written.
    Ok(NoteDoc::from_bytes(0, &document)?
        .attachments()
        .iter()
        .flat_map(crate::files::Attachment::parts)
        .find(|part| part.file_id == file))
}

/// References the server has not taken, for notes the server holds. A
/// reference to a note it does not hold yet would be refused, and waits
/// until the note has been sent.
pub(super) fn to_confirm(connection: &Connection, after: i64, max: usize) -> Result<Vec<FileLink>> {
    links(
        connection,
        "SELECT l.rowid, l.file_id, l.note_id \
           FROM file_links l JOIN notes n ON n.id = l.note_id \
          WHERE l.confirmed = 0 AND l.unnamed_at IS NULL \
            AND n.server_change > 0 AND l.rowid > ?1 \
          ORDER BY l.rowid LIMIT ?2",
        after,
        max,
    )
}

pub(super) fn mark_confirmed(connection: &mut Connection, link: &FileLink) -> Result<()> {
    let tx = write_transaction(connection)?;
    tx.execute(
        "UPDATE file_links SET confirmed = 1, problem = NULL \
          WHERE file_id = ?1 AND note_id = ?2",
        params![link.file_id.to_string(), link.note_id.as_str()],
    )?;
    tx.commit()?;
    Ok(())
}

pub(super) fn set_problem(
    connection: &mut Connection,
    link: &FileLink,
    reason: &str,
) -> Result<()> {
    let tx = write_transaction(connection)?;
    tx.execute(
        "UPDATE file_links SET problem = ?3 WHERE file_id = ?1 AND note_id = ?2",
        params![link.file_id.to_string(), link.note_id.as_str(), reason],
    )?;
    tx.commit()?;
    Ok(())
}

pub(super) fn problems(connection: &Connection) -> Result<Vec<FileProblem>> {
    let mut statement = connection.prepare(
        "SELECT file_id, note_id, problem FROM file_links \
          WHERE problem IS NOT NULL AND unnamed_at IS NULL ORDER BY rowid",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut problems = Vec::new();
    for row in rows {
        let (file, note, reason) = row?;
        problems.push(FileProblem {
            file_id: file_id(&file)?,
            note_id: note_id(&note)?,
            reason,
        });
    }
    Ok(problems)
}

/// References to drop on the server. The rule is the one for purging a
/// note: this device has itself seen the note without the file since
/// before `unnamed_before_ms`, and the server holds the note exactly as
/// this device does, so nothing unsent could name the file again.
///
/// Whether the server ever took the reference does not matter. Its answer
/// to a confirm may have been lost, and dropping a reference it does not
/// have changes nothing.
pub(super) fn to_release(
    connection: &Connection,
    unnamed_before_ms: i64,
    max: usize,
) -> Result<Vec<FileLink>> {
    links(
        connection,
        "SELECT l.rowid, l.file_id, l.note_id \
           FROM file_links l JOIN notes n ON n.id = l.note_id \
          WHERE l.unnamed_at <= ?1 \
            AND n.pushed_revision = n.revision AND n.server_change > 0 \
            AND n.id NOT IN (SELECT note_id FROM sync_problems) \
          ORDER BY l.rowid LIMIT ?2",
        unnamed_before_ms,
        max,
    )
}

/// Forgets a reference the server has dropped. If the note has come to
/// name the file again since the reference was listed, the row stays and
/// the reference is confirmed again.
pub(super) fn forget(connection: &mut Connection, link: &FileLink) -> Result<()> {
    let tx = write_transaction(connection)?;
    let pair = params![link.file_id.to_string(), link.note_id.as_str()];
    tx.execute(
        "DELETE FROM file_links \
          WHERE file_id = ?1 AND note_id = ?2 AND unnamed_at IS NOT NULL",
        pair,
    )?;
    tx.execute(
        "UPDATE file_links SET confirmed = 0 WHERE file_id = ?1 AND note_id = ?2",
        pair,
    )?;
    tx.commit()?;
    Ok(())
}

/// The files with a reference the server has not taken and has not
/// refused.
pub(super) fn unconfirmed(connection: &Connection) -> Result<Vec<Uuid>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT file_id FROM file_links \
          WHERE confirmed = 0 AND unnamed_at IS NULL AND problem IS NULL",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut files = Vec::new();
    for row in rows {
        files.push(file_id(&row?)?);
    }
    Ok(files)
}

/// The files a note names that the server has not been seen to take,
/// whether or not it refused them. A copy of one on this device may be
/// the only copy there is.
pub(super) fn not_taken(connection: &Connection) -> Result<Vec<Uuid>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT file_id FROM file_links WHERE confirmed = 0 AND unnamed_at IS NULL",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut files = Vec::new();
    for row in rows {
        files.push(file_id(&row?)?);
    }
    Ok(files)
}

/// Removes from `shelf` the files that have been unneeded since before
/// `unneeded_before_ms`, and notes at `now_ms` those seen unneeded for
/// the first time. Returns how many were removed.
///
/// A file is needed while a note names it, deleted or not, and while a
/// reference to it is left to tell the server about.
pub(super) fn clear_spare(
    connection: &mut Connection,
    shelf: &Shelf,
    now_ms: i64,
    unneeded_before_ms: i64,
) -> Result<usize> {
    let held = shelf.list()?;
    let tx = write_transaction(connection)?;
    let needed: HashSet<String> = {
        let mut statement =
            tx.prepare("SELECT file_id FROM note_files UNION SELECT file_id FROM file_links")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut spare = HashSet::new();
    for file in held {
        let key = file.to_string();
        if !needed.contains(&key) {
            tx.execute(
                "INSERT OR IGNORE INTO shelf_spare (file_id, since) VALUES (?1, ?2)",
                params![key, now_ms],
            )?;
            spare.insert(key);
        }
    }
    let noted: Vec<(String, i64)> = {
        let mut statement = tx.prepare("SELECT file_id, since FROM shelf_spare")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut removed = 0;
    for (key, since) in noted {
        let still_spare = spare.contains(&key);
        if still_spare && since > unneeded_before_ms {
            continue;
        }
        if still_spare {
            shelf.remove(file_id(&key)?)?;
            removed += 1;
        }
        // Removed, needed again, or no longer on the shelf.
        tx.execute("DELETE FROM shelf_spare WHERE file_id = ?1", [&key])?;
    }
    tx.commit()?;
    Ok(removed)
}
