//! Reading and writing notes.
//!
//! A note's row holds its CRDT document and five things derived from it: the
//! text, the last-edited time, whether it is deleted, whether it is
//! archived, and (in two other tables) its tags and its entry in the
//! full-text index. Every write loads
//! the document, changes it and saves it with its derived values inside one
//! `BEGIN IMMEDIATE` transaction, so the derived values can never disagree
//! with the document, and two processes can never interleave on one note.

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};

use dirt_proto::api::limits;

use super::schema::{client_id, write_transaction};
use super::sync_state::remove_row;
use super::tags::{Chosen, TagRule};
use crate::files::Attachment;
use crate::models::{extract_tags, Note, NoteId, PileNote, Voice, VoiceMark};
use crate::notes::{merge3, NoteDoc};
use crate::{Error, Result};

const NOTE_COLUMNS: &str = "n.id, n.text, n.edited_at, n.archived, n.voice";

fn note_from_row(row: &Row<'_>) -> rusqlite::Result<Note> {
    let id: String = row.get(0)?;
    let id: NoteId = id.parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let edited_at: i64 = row.get(2)?;
    Ok(Note {
        id,
        content: row.get(1)?,
        // Creation time is in the identifier. An identifier that carries no
        // time can only have been made outside this program.
        created_at: id.created_at_ms().unwrap_or(edited_at),
        updated_at: edited_at,
        archived: row.get(3)?,
        voice: row
            .get::<_, Option<String>>(4)?
            .as_deref()
            .and_then(VoiceMark::from_entry)
            .map(Voice::from),
    })
}

/// Where a document being saved came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    /// An edit made on this device, or a delete. It raises the note's
    /// revision, which marks the note as unsent.
    Local,
    /// A change made on this device that is not an edit: archiving a note,
    /// or bringing it back to the list. It raises the revision as an edit
    /// does, so the note is unsent and is pushed like any other. It leaves
    /// alone the revision at which the note was last edited.
    Mark,
    /// A document merged in from the server. The revision is left alone:
    /// what the server already has does not need sending.
    Remote,
}

/// A note's row number, document and last change number seen, or `None`
/// if there is no such note.
pub(super) fn load_for_update(
    tx: &Transaction<'_>,
    id: &NoteId,
) -> Result<Option<(i64, Vec<u8>, i64)>> {
    Ok(tx
        .query_row(
            "SELECT rowid, doc, server_change FROM notes WHERE id = ?1",
            [id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

/// A note's row number and document, or `None` if there is no such note.
fn load(tx: &Transaction<'_>, id: &NoteId) -> Result<Option<(i64, Vec<u8>)>> {
    Ok(tx
        .query_row(
            "SELECT rowid, doc FROM notes WHERE id = ?1",
            [id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

/// Writes a document and everything derived from it. Returns the note, or
/// `None` if the document says the note is deleted.
pub(super) fn save(
    tx: &Transaction<'_>,
    id: &NoteId,
    doc: &NoteDoc,
    existing_row: Option<i64>,
    origin: Origin,
) -> Result<Option<Note>> {
    let key = id.as_str();
    let text = doc.text();
    let deleted = doc.is_deleted();
    let archived = doc.is_archived();
    let voice = doc.voice();
    let voice_entry = voice.as_ref().map(VoiceMark::to_entry);
    // A document always has an edit time once it has been edited. One that
    // has not, which only a merge of an empty document could produce, falls
    // back to the creation time.
    let edited_at = doc
        .edited_at_ms()
        .or_else(|| id.created_at_ms())
        .unwrap_or_default();
    let bytes = doc.encode();

    // Made on this device, and so to be sent.
    let local = origin != Origin::Remote;
    let edit = origin == Origin::Local;
    // When this device first saw the note as deleted: kept while it stays
    // deleted, and forgotten if it comes back.
    let now = crate::util::now_ms();
    let row = if let Some(row) = existing_row {
        // Every value on the right is the row's as it was, so `revision + 1`
        // is the revision this very write gives the note. A document that
        // came from the server shows that the server has the note, whatever
        // the row said: a database put back from a copy made before the
        // note's first push says it was never offered.
        tx.execute(
            "UPDATE notes SET doc = ?1, text = ?2, edited_at = ?3, deleted = ?4, \
                    revision = revision + ?6, \
                    edited_revision = CASE WHEN ?8 THEN revision + 1 ELSE edited_revision END, \
                    deleted_at = CASE WHEN ?4 THEN COALESCE(deleted_at, ?7) END, \
                    archived = ?9, \
                    offered = offered OR ?10, \
                    voice = ?11 \
              WHERE rowid = ?5",
            params![
                bytes,
                text,
                edited_at,
                deleted,
                row,
                i64::from(local),
                now,
                edit,
                archived,
                !local,
                voice_entry
            ],
        )?;
        row
    } else {
        // A note that arrives from the server starts out as sent, with no
        // edit of this device's in it, and the server has it. One made
        // here has yet to be offered.
        let pushed_revision = if local { None } else { Some(1) };
        let deleted_at = deleted.then_some(now);
        tx.execute(
            "INSERT INTO notes (id, doc, text, edited_at, deleted, revision, pushed_revision, \
                                deleted_at, archived, edited_revision, offered, voice) \
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                key,
                bytes,
                text,
                edited_at,
                deleted,
                pushed_revision,
                deleted_at,
                archived,
                i64::from(local),
                !local,
                voice_entry
            ],
        )?;
        tx.last_insert_rowid()
    };

    if local {
        // A problem such as "too large" may be cured by this very edit,
        // so the note gets another try.
        tx.execute(
            "DELETE FROM sync_problems WHERE note_id = ?1 AND retry_after_edit = 1",
            [&key],
        )?;
    }

    // The files the note names, deleted or not: a deleted note can come
    // back, and its files with it.
    // A thumbnail is a file of its own, kept and sent like any other.
    tx.execute("DELETE FROM note_files WHERE note_id = ?1", [&key])?;
    for file in doc.named_files() {
        tx.execute(
            "INSERT OR IGNORE INTO note_files (note_id, file_id) VALUES (?1, ?2)",
            params![key, file.to_string()],
        )?;
    }
    // What the server has to be told follows what the note names. A file
    // named for the first time has a reference to confirm. A file no
    // longer named is dated, so that the server can be told after a wait.
    // One that is named again loses the date, and is to be confirmed
    // again: another device that saw it removed may have told the server
    // so in the meantime, and the server would then delete a file that a
    // note names.
    tx.execute(
        "INSERT OR IGNORE INTO file_links (file_id, note_id) \
         SELECT file_id, note_id FROM note_files WHERE note_id = ?1",
        [&key],
    )?;
    tx.execute(
        "UPDATE file_links \
            SET confirmed = CASE \
                  WHEN unnamed_at IS NOT NULL \
                   AND file_id IN (SELECT file_id FROM note_files WHERE note_id = ?1) THEN 0 \
                  ELSE confirmed END, \
                unnamed_at = CASE \
                  WHEN file_id IN (SELECT file_id FROM note_files WHERE note_id = ?1) THEN NULL \
                  ELSE COALESCE(unnamed_at, ?2) END \
          WHERE note_id = ?1",
        params![key, now],
    )?;

    // The search index and the tags hold live notes only.
    tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", [row])?;
    tx.execute("DELETE FROM note_tags WHERE note_id = ?1", [&key])?;
    if deleted {
        return Ok(None);
    }
    tx.execute(
        "INSERT INTO notes_fts (rowid, text) VALUES (?1, ?2)",
        params![row, text],
    )?;
    for tag in extract_tags(&text) {
        tx.execute(
            "INSERT OR IGNORE INTO note_tags (note_id, tag) VALUES (?1, ?2)",
            params![key, tag],
        )?;
    }
    Ok(Some(Note {
        id: *id,
        content: text,
        created_at: id.created_at_ms().unwrap_or(edited_at),
        updated_at: edited_at,
        archived,
        voice: voice.map(Voice::from),
    }))
}

fn not_found(id: &NoteId) -> Error {
    Error::NotFound(id.to_string())
}

pub(super) fn create(
    connection: &mut Connection,
    id: NoteId,
    text: &str,
    now_ms: i64,
) -> Result<Note> {
    create_if_new(connection, id, text, now_ms)?
        .ok_or_else(|| Error::InvalidInput(format!("note {id} already exists")))
}

/// Makes the note, unless a note has this ID already, deleted or not:
/// then `None`, and nothing is written. The check and the making are one
/// transaction, so two writers taking the same note in make it once.
pub(super) fn create_if_new(
    connection: &mut Connection,
    id: NoteId,
    text: &str,
    now_ms: i64,
) -> Result<Option<Note>> {
    let tx = write_transaction(connection)?;
    if load(&tx, &id)?.is_some() {
        return Ok(None);
    }
    let doc = NoteDoc::new(client_id(&tx)?);
    doc.set_text(text, now_ms);
    if text.is_empty() {
        // An empty text is "no change" to a new document, so record the
        // creation as an edit to give the note an edit time.
        doc.restore(now_ms);
    }
    let note = save(&tx, &id, &doc, None, Origin::Local)?.ok_or_else(|| not_found(&id))?;
    tx.commit()?;
    Ok(Some(note))
}

pub(super) fn update(
    connection: &mut Connection,
    id: &NoteId,
    base: Option<&str>,
    text: &str,
    now_ms: i64,
) -> Result<Note> {
    let tx = write_transaction(connection)?;
    let (row, bytes) = load(&tx, id)?.ok_or_else(|| not_found(id))?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Err(not_found(id));
    }
    // Read inside the transaction that writes, so nothing comes between
    // the text the changes are made to and the writing of them.
    let text = base.map_or_else(|| text.to_owned(), |base| merge3(base, text, &doc.text()));
    if !doc.set_text(&text, now_ms) {
        // Nothing changed: leave the row, and its revision, alone.
        drop(tx);
        return get(connection, id)?.ok_or_else(|| not_found(id));
    }
    let note = save(&tx, id, &doc, Some(row), Origin::Local)?.ok_or_else(|| not_found(id))?;
    tx.commit()?;
    Ok(note)
}

/// The files attached to a note, in the order they were attached. A
/// deleted note has them still.
pub(super) fn attachments(connection: &Connection, id: &NoteId) -> Result<Vec<Attachment>> {
    let document: Option<Vec<u8>> = connection
        .query_row(
            "SELECT doc FROM notes WHERE id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let document = document.ok_or_else(|| not_found(id))?;
    // Reading needs no client ID of its own: nothing is written.
    Ok(NoteDoc::from_bytes(0, &document)?.attachments())
}

/// Every file that some note names, a deleted note included. A file on the
/// shelf that is not among these is needed by nothing on this device.
pub(super) fn files_in_use(connection: &Connection) -> Result<Vec<uuid::Uuid>> {
    let mut statement =
        connection.prepare("SELECT DISTINCT file_id FROM note_files ORDER BY file_id")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut files = Vec::new();
    for row in rows {
        let id = row?;
        files.push(
            id.parse().map_err(|_| {
                Error::Database(format!("the store holds an invalid file ID: {id}"))
            })?,
        );
    }
    Ok(files)
}

/// Names a file in a note's document. The file itself is on the shelf.
/// Returns the attachment as the note has it: without its thumbnail, if
/// the note had room for the file and not for both.
///
/// A note's files are counted as the server counts them: every file the
/// note names, a thumbnail being one.
pub(super) fn attach(
    connection: &mut Connection,
    id: &NoteId,
    mut attachment: Attachment,
    now_ms: i64,
) -> Result<Attachment> {
    let tx = write_transaction(connection)?;
    let (row, bytes) = load(&tx, id)?.ok_or_else(|| not_found(id))?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Err(not_found(id));
    }
    let most = usize::try_from(limits::MAX_FILES_PER_NOTE).unwrap_or(usize::MAX);
    let room = most.saturating_sub(doc.named_files().len());
    if room == 0 {
        return Err(Error::InvalidInput(format!(
            "a note may have at most {most} files, and a picture's preview counts as one"
        )));
    }
    if room == 1 {
        attachment.thumbnail = None;
    }
    if !doc.attach(&attachment, now_ms) {
        return Err(Error::InvalidInput(
            "the note already has this file".to_owned(),
        ));
    }
    save(&tx, id, &doc, Some(row), Origin::Local)?;
    tx.commit()?;
    Ok(attachment)
}

/// Removes a file from a note's document. Returns whether the note named
/// it. The file itself stays on the shelf until nothing needs it.
pub(super) fn detach(
    connection: &mut Connection,
    id: &NoteId,
    file_id: uuid::Uuid,
    now_ms: i64,
) -> Result<bool> {
    let tx = write_transaction(connection)?;
    let (row, bytes) = load(&tx, id)?.ok_or_else(|| not_found(id))?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Err(not_found(id));
    }
    if !doc.detach(file_id, now_ms) {
        return Ok(false);
    }
    save(&tx, id, &doc, Some(row), Origin::Local)?;
    // A recording added to the note and taken off before its words were
    // made: they are not made.
    tx.execute(
        "DELETE FROM voice_added WHERE file_id = ?1 AND note_id = ?2",
        params![file_id.to_string(), id.as_str()],
    )?;
    tx.commit()?;
    Ok(true)
}

pub(super) fn delete(connection: &mut Connection, id: &NoteId) -> Result<()> {
    let tx = write_transaction(connection)?;
    let (row, bytes) = load(&tx, id)?.ok_or_else(|| not_found(id))?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Ok(());
    }
    doc.delete();
    save(&tx, id, &doc, Some(row), Origin::Local)?;
    tx.commit()?;
    Ok(())
}

pub(super) fn restore(connection: &mut Connection, id: &NoteId, now_ms: i64) -> Result<Note> {
    let tx = write_transaction(connection)?;
    let (row, bytes) = load(&tx, id)?.ok_or_else(|| not_found(id))?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if !doc.is_deleted() {
        drop(tx);
        return get(connection, id)?.ok_or_else(|| not_found(id));
    }
    doc.bring_back(now_ms);
    let note = save(&tx, id, &doc, Some(row), Origin::Local)?.ok_or_else(|| not_found(id))?;
    tx.commit()?;
    Ok(note)
}

/// Archives a note, or brings it back to the list. Returns the note.
///
/// Not an edit: the note keeps its edit time. It is unsent all the same,
/// because the row's revision rises with every change made here.
pub(super) fn set_archived(
    connection: &mut Connection,
    id: &NoteId,
    archived: bool,
) -> Result<Note> {
    let tx = write_transaction(connection)?;
    let (row, bytes) = load(&tx, id)?.ok_or_else(|| not_found(id))?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Err(not_found(id));
    }
    if !doc.set_archived(archived) {
        // Already as asked: leave the row, and its revision, alone.
        drop(tx);
        return get(connection, id)?.ok_or_else(|| not_found(id));
    }
    let note = save(&tx, id, &doc, Some(row), Origin::Mark)?.ok_or_else(|| not_found(id))?;
    tx.commit()?;
    Ok(note)
}

/// What became of a note that was to be let go if it was empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LetGo {
    /// The note has text or a file. It stays as it is.
    Kept,
    /// The note had never been offered to the server. It is off the device
    /// altogether: no row, no document, nothing to sync.
    Dropped,
    /// The server may have the note, so it was deleted as any note is, and
    /// the delete syncs.
    Deleted,
    /// There was nothing to let go: the store holds no such note, or the
    /// note is deleted already.
    Gone,
}

/// Lets go of a note that holds nothing: no text but spaces and line
/// breaks, and no file.
///
/// The interface makes a note at the first character typed, and a person
/// who takes the character back has written nothing. If no push has ever
/// held the note, it exists on this device alone and its row is removed.
/// If one has, another device may hold the note, and it is deleted as any
/// note is.
pub(super) fn let_go_if_empty(connection: &mut Connection, id: &NoteId) -> Result<LetGo> {
    let tx = write_transaction(connection)?;
    let held: Option<(i64, Vec<u8>, bool)> = tx
        .query_row(
            "SELECT rowid, doc, offered FROM notes WHERE id = ?1",
            [id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((row, bytes, offered)) = held else {
        return Ok(LetGo::Gone);
    };
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Ok(LetGo::Gone);
    }
    // A file this version cannot read is a file all the same.
    if !doc.text().trim().is_empty() || !doc.named_files().is_empty() {
        return Ok(LetGo::Kept);
    }
    let outcome = if offered {
        doc.delete();
        save(&tx, id, &doc, Some(row), Origin::Local)?;
        LetGo::Deleted
    } else {
        remove_row(&tx, row, id)?;
        LetGo::Dropped
    };
    tx.commit()?;
    Ok(outcome)
}

/// Merges another copy of a note's document into this store, creating the
/// note if it is not here yet. Returns the note, or `None` if the merged
/// document says it is deleted.
///
/// Merging the same document twice changes nothing, so this is safe to
/// repeat.
pub(super) fn merge_document(
    connection: &mut Connection,
    id: &NoteId,
    incoming: &[u8],
) -> Result<Option<Note>> {
    let tx = write_transaction(connection)?;
    let me = client_id(&tx)?;
    let note = match load(&tx, id)? {
        None => {
            let doc = NoteDoc::from_bytes(me, incoming)?;
            save(&tx, id, &doc, None, Origin::Local)?
        }
        Some((row, mine)) => {
            let doc = NoteDoc::from_bytes(me, &mine)?;
            doc.merge(incoming)?;
            if doc.encode() == mine {
                // The incoming copy held nothing new.
                drop(tx);
                return get(connection, id);
            }
            save(&tx, id, &doc, Some(row), Origin::Local)?
        }
    };
    tx.commit()?;
    Ok(note)
}

/// One note's document as it stood at one revision.
pub(super) struct Snapshot {
    pub(super) id: NoteId,
    pub(super) document: Vec<u8>,
}

/// Every note's document, deleted notes included.
pub(super) fn snapshots(connection: &Connection) -> Result<Vec<Snapshot>> {
    let mut statement = connection.prepare("SELECT id, doc FROM notes ORDER BY rowid")?;
    let rows = statement.query_map([], |row| {
        let id: String = row.get(0)?;
        let document: Vec<u8> = row.get(1)?;
        Ok((id, document))
    })?;
    let mut snapshots = Vec::new();
    for row in rows {
        let (id, document) = row?;
        let id = id
            .parse()
            .map_err(|_| Error::Database(format!("the store holds an invalid note ID: {id}")))?;
        snapshots.push(Snapshot { id, document });
    }
    Ok(snapshots)
}

/// Removes each listed note, with its tags and search entry, but only if it
/// still holds exactly the listed document. A note changed since then
/// stays. Returns how many were removed.
///
/// The document is compared, not the revision: a merge of a document from
/// the server changes a note without raising its revision.
pub(super) fn remove_unchanged(
    connection: &mut Connection,
    notes: &[(NoteId, Vec<u8>)],
) -> Result<usize> {
    let tx = write_transaction(connection)?;
    let mut removed = 0;
    for (id, document) in notes {
        let row: Option<i64> = tx
            .query_row(
                "SELECT rowid FROM notes WHERE id = ?1 AND doc = ?2",
                params![id.as_str(), document],
                |row| row.get(0),
            )
            .optional()?;
        let Some(row) = row else { continue };
        tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", [row])?;
        // The note's tag rows go with it, through the foreign key.
        tx.execute("DELETE FROM notes WHERE rowid = ?1", [row])?;
        removed += 1;
    }
    tx.commit()?;
    Ok(removed)
}

/// How many notes are not deleted.
pub(super) fn count(connection: &Connection) -> Result<usize> {
    let count: i64 =
        connection.query_row("SELECT count(*) FROM notes WHERE deleted = 0", [], |row| {
            row.get(0)
        })?;
    Ok(usize::try_from(count).unwrap_or_default())
}

/// How many notes are in the list, and how many are archived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoteCounts {
    /// Notes that are neither archived nor deleted: what the list holds.
    pub listed: usize,
    /// Archived notes that are not deleted.
    pub archived: usize,
    /// A number that changes when a note is made, deleted or brought
    /// back: the sum of the row numbers of the notes that are not
    /// deleted. An edit or an archive leaves it as it is. With the counts,
    /// it tells whether the notes the pile is made of may have changed
    /// ([`crate::Store::pile`] reads every one of them). Two such changes
    /// between two readings could cancel out only if a note brought back
    /// had the very row number of one deleted.
    pub live_mark: i64,
}

/// The statement behind [`counts`]: read in the list's index alone, which
/// holds whether a note is deleted, whether it is archived and its row
/// number. No note's own row is read.
const COUNTS_SQL: &str = "SELECT count(*), sum(archived), sum(rowid) FROM notes WHERE deleted = 0";

pub(super) fn counts(connection: &Connection) -> Result<NoteCounts> {
    let (all, archived, mark): (i64, Option<i64>, Option<i64>) =
        connection.query_row(COUNTS_SQL, [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    let archived = archived.unwrap_or_default();
    Ok(NoteCounts {
        listed: usize::try_from(all - archived).unwrap_or_default(),
        archived: usize::try_from(archived).unwrap_or_default(),
        live_mark: mark.unwrap_or_default(),
    })
}

pub(super) fn get(connection: &Connection, id: &NoteId) -> Result<Option<Note>> {
    Ok(connection
        .query_row(
            &format!("SELECT {NOTE_COLUMNS} FROM notes n WHERE n.id = ?1 AND n.deleted = 0"),
            [id.as_str()],
            note_from_row,
        )
        .optional()?)
}

fn collect(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<Note>>,
) -> Result<Vec<Note>> {
    let mut notes = Vec::new();
    for row in rows {
        notes.push(row?);
    }
    Ok(notes)
}

fn to_sql_count(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// Which of the live notes a list holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Which {
    /// The list: the notes that are not archived.
    Listed,
    /// The archived notes alone.
    Archived,
    /// Both, the archived notes after the rest: what Find looks through,
    /// and what an export holds.
    Every,
}

impl Which {
    /// What this adds to `WHERE n.deleted = 0`.
    const fn narrows(self) -> &'static str {
        match self {
            Self::Listed => "AND n.archived = 0",
            Self::Archived => "AND n.archived = 1",
            Self::Every => "",
        }
    }
}

/// Live notes, most recently edited first.
pub(super) fn list(
    connection: &Connection,
    which: Which,
    limit: usize,
    offset: usize,
) -> Result<Vec<Note>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {NOTE_COLUMNS} FROM notes n WHERE n.deleted = 0 {} \
          ORDER BY n.archived, n.edited_at DESC, n.id DESC LIMIT ?1 OFFSET ?2",
        which.narrows()
    ))?;
    let rows = statement.query_map(
        params![to_sql_count(limit), to_sql_count(offset)],
        note_from_row,
    )?;
    collect(rows)
}

/// Live notes that pass the chosen tags' rule, most recently edited
/// first. With no tag chosen, [`list`].
pub(super) fn list_by_tags(
    connection: &Connection,
    which: Which,
    chosen: &Chosen,
    limit: usize,
    offset: usize,
) -> Result<Vec<Note>> {
    if chosen.is_empty() {
        return list(connection, which, limit, offset);
    }
    let after = chosen.len();
    let mut statement = connection.prepare(&format!(
        "SELECT {NOTE_COLUMNS} {} AND n.deleted = 0 {} \
          ORDER BY n.archived, n.edited_at DESC, n.id DESC LIMIT ?{} OFFSET ?{}",
        chosen.notes_from_tags(),
        which.narrows(),
        after + 1,
        after + 2
    ))?;
    let mut parameters = chosen.parameters();
    parameters.push(Value::Integer(to_sql_count(limit)));
    parameters.push(Value::Integer(to_sql_count(offset)));
    let rows = statement.query_map(params_from_iter(parameters), note_from_row)?;
    collect(rows)
}

/// Every tag in use, with the number of live notes that carry it. An
/// archived note counts: a tag is a way to find a note, and Find keeps the
/// archived ones, so the number beside a tag is the number of notes that
/// choosing it shows.
pub(super) fn list_tags(connection: &Connection) -> Result<Vec<(String, usize)>> {
    let mut statement =
        connection.prepare("SELECT tag, count(*) FROM note_tags GROUP BY tag ORDER BY tag")?;
    let rows = statement.query_map([], |row| {
        let tag: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok((tag, usize::try_from(count).unwrap_or_default()))
    })?;
    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

/// The IDs of notes whose ID starts with `prefix`, for commands that accept
/// a shortened ID: the live notes, or with `deleted` the deleted ones this
/// device still holds, which is what a command that brings one back needs.
pub(super) fn ids_by_prefix(
    connection: &Connection,
    prefix: &str,
    deleted: bool,
    limit: usize,
) -> Result<Vec<String>> {
    // IDs are hexadecimal digits and hyphens. Anything else cannot match, and
    // refusing it keeps `%` and `_` out of the LIKE pattern.
    let prefix = prefix.trim().to_lowercase();
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare("SELECT id FROM notes WHERE deleted = ?3 AND id LIKE ?1 ORDER BY id LIMIT ?2")?;
    let rows = statement.query_map(
        params![format!("{prefix}%"), to_sql_count(limit), deleted],
        |row| row.get::<_, String>(0),
    )?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row?);
    }
    Ok(ids)
}

/// Turns what a person typed into a full-text query: every word must appear,
/// and each word also matches as the start of a longer one.
///
/// Each word is quoted, so punctuation in the input is never read as query
/// syntax. Returns `None` when the input holds no letter or digit.
pub(super) fn full_text_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// Live notes matching every word of `query`, best match first, the
/// archived ones after the rest. A query with no letter or digit matches
/// nothing.
pub(super) fn search(connection: &Connection, query: &str, limit: usize) -> Result<Vec<Note>> {
    if full_text_query(query).is_none() {
        return Ok(Vec::new());
    }
    find(
        connection,
        query,
        &Chosen::new::<&str>(&[], TagRule::All),
        limit,
    )
}

/// The statement that finds the notes matching a full-text query and
/// passing the chosen tags' rule, best match first. An archived note is
/// found too, after every note that is not archived. Its parameters are
/// the chosen tags, then the query, then the limit.
///
/// `CROSS JOIN` is SQLite's way of fixing the order of a join: the table
/// on its left is read first. Left to choose, SQLite reads `notes` first
/// and asks the index about each note in turn, and each such question
/// walks the whole list of notes that hold the word. With 20,000 notes
/// holding the word typed, that took 22 seconds. Read first, the index
/// answers once.
///
/// The index is read first with tags chosen too, then the note, then its
/// tags: each chosen tag is one look into the tags' table by its key.
fn search_sql(chosen: &Chosen) -> String {
    let after = chosen.len();
    let tagged = if chosen.is_empty() {
        String::new()
    } else {
        format!("AND {} ", chosen.holds("n.id"))
    };
    format!(
        "SELECT {NOTE_COLUMNS} FROM notes_fts f \
           CROSS JOIN notes n ON n.rowid = f.rowid \
          WHERE notes_fts MATCH ?{} AND n.deleted = 0 {tagged}\
          ORDER BY n.archived, f.rank, n.edited_at DESC LIMIT ?{}",
        after + 1,
        after + 2
    )
}

/// Live notes that match every word of `query` and pass the chosen tags'
/// rule. Either may be left out: an empty query matches every note, and no
/// tag means any.
///
/// With words to match, the best match comes first; without, the most
/// recently edited. A query with text but no letter or digit matches
/// nothing.
///
/// With neither words nor a tag this is the list, which leaves archived
/// notes out. With either, something is being found, and the archived
/// notes that match come after the rest.
pub(super) fn find(
    connection: &Connection,
    query: &str,
    chosen: &Chosen,
    limit: usize,
) -> Result<Vec<Note>> {
    let Some(words) = full_text_query(query) else {
        if !query.trim().is_empty() {
            return Ok(Vec::new());
        }
        return if chosen.is_empty() {
            list(connection, Which::Listed, limit, 0)
        } else {
            list_by_tags(connection, Which::Every, chosen, limit, 0)
        };
    };
    let mut statement = connection.prepare(&search_sql(chosen))?;
    let mut parameters = chosen.parameters();
    parameters.push(Value::Text(words));
    parameters.push(Value::Integer(to_sql_count(limit)));
    let rows = statement.query_map(params_from_iter(parameters), note_from_row)?;
    collect(rows)
}

/// The statement behind [`pile`].
///
/// It is read by the index that serves the list, and the index is named
/// so that this does not rest on SQLite choosing it. The index holds the
/// edit time and whether the note is archived. Only the ID is taken from
/// the note's own row, where it comes first, before the document and the
/// text, so neither of those is read however long it is. Read from end
/// to end instead, the table gives up the edit time only after the whole
/// of each note: for 20,000 notes of 2,000 characters that took fifteen
/// times as long (`docs/LIMITS.md`).
const PILE_SQL: &str =
    "SELECT id, edited_at, archived FROM notes INDEXED BY notes_by_edit WHERE deleted = 0";

/// Every live note, archived or not, as the pile needs it: oldest first,
/// which for IDs that begin with their time is the order of the IDs.
///
/// The documents and the text are neither asked for nor read: see
/// [`PILE_SQL`].
pub(super) fn pile(connection: &Connection) -> Result<Vec<PileNote>> {
    let mut statement = connection.prepare(PILE_SQL)?;
    let mut rows = statement.query([])?;
    let mut notes = Vec::new();
    while let Some(row) = rows.next()? {
        // Read as it is stored, without a copy: there may be 20,000.
        let id = row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?;
        let id: NoteId = id
            .parse()
            .map_err(|_| Error::Database(format!("the store holds an invalid note ID: {id}")))?;
        notes.push(PileNote {
            id,
            edited_at: row.get(1)?,
            archived: row.get(2)?,
        });
    }
    // They come in the order of the list, newest edit first.
    notes.sort_unstable_by_key(|note| note.id);
    Ok(notes)
}

/// The local revision counter of a note, deleted or not. Step 5's sync reads
/// it to tell whether a note changed while a push was in flight.
pub(super) fn revision(connection: &Connection, id: &NoteId) -> Result<Option<i64>> {
    Ok(connection
        .query_row(
            "SELECT revision FROM notes WHERE id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables a statement reads, in the order SQLite reads them.
    fn read_order(connection: &Connection, sql: &str, parameters: usize) -> Vec<String> {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        let values = vec![Value::Text("\"word\"*".to_owned()); parameters];
        let steps = statement
            .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
            .unwrap();
        steps
            .map(|step| step.expect("a step of the plan"))
            .filter(|step| step.starts_with("SCAN") || step.starts_with("SEARCH"))
            .collect()
    }

    fn empty_database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut connection =
            super::super::schema::open_connection(&folder.path().join("notes.db")).unwrap();
        super::super::schema::set_up(&mut connection).unwrap();
        (folder, connection)
    }

    /// A search has to begin at the full-text index. Begun at the notes,
    /// it asks the index once for every note, which with many notes takes
    /// many seconds.
    #[test]
    fn a_search_reads_the_full_text_index_first() {
        let (_folder, connection) = empty_database();

        let none = Chosen::new::<&str>(&[], TagRule::All);
        let plain = read_order(&connection, &search_sql(&none), 2);
        assert!(
            plain[0].contains("SCAN f VIRTUAL TABLE"),
            "the search reads in this order: {plain:?}"
        );
        assert!(plain[1].starts_with("SEARCH n "), "{plain:?}");
        assert_eq!(plain.len(), 2, "{plain:?}");

        // With tags chosen, under either rule: the index, the note, and
        // then the note's tags by their key.
        for chosen in [
            Chosen::new(&["a"], TagRule::All),
            Chosen::new(&["a", "b"], TagRule::All),
            Chosen::new(&["a", "b"], TagRule::Any),
        ] {
            let tagged = read_order(&connection, &search_sql(&chosen), chosen.len() + 2);
            assert!(
                tagged[0].contains("SCAN f VIRTUAL TABLE"),
                "the search with {chosen:?} reads in this order: {tagged:?}"
            );
            assert!(tagged[1].starts_with("SEARCH n "), "{tagged:?}");
            assert!(
                tagged[2..]
                    .iter()
                    .all(|step| step.starts_with("SEARCH note_tags ")),
                "{tagged:?}"
            );
            assert!(tagged.len() > 2, "{tagged:?}");
        }
    }

    /// A list narrowed to tags is read in the order of the list, so that
    /// it stops at its limit, and each note is looked up among the tags
    /// by key. No table is read from end to end to answer it.
    #[test]
    fn a_list_narrowed_to_tags_is_read_in_its_own_order() {
        let (_folder, connection) = empty_database();
        for chosen in [
            Chosen::new(&["a"], TagRule::All),
            Chosen::new(&["a", "b"], TagRule::All),
            Chosen::new(&["a", "b"], TagRule::Any),
        ] {
            let sql = format!(
                "SELECT {NOTE_COLUMNS} {} AND n.deleted = 0 \
                  ORDER BY n.archived, n.edited_at DESC, n.id DESC LIMIT 5",
                chosen.notes_from_tags()
            );
            let plan = read_order(&connection, &sql, chosen.len());
            let said = format!("{chosen:?} reads in this order: {plan:?}");
            assert!(
                plan[0].starts_with("SEARCH n USING INDEX notes_by_edit"),
                "{said}"
            );
            assert!(!plan.iter().any(|step| step.starts_with("SCAN")), "{said}");
            assert!(
                plan[1..]
                    .iter()
                    .all(|step| step.contains("note_tags_by_tag (tag=?")),
                "{said}"
            );
        }
    }

    /// The counts read the list's index and nothing else: they are read
    /// with every reading of the list.
    #[test]
    fn the_counts_are_read_from_the_lists_index_alone() {
        let (_folder, connection) = empty_database();
        assert_eq!(
            read_order(&connection, COUNTS_SQL, 0),
            ["SEARCH notes USING COVERING INDEX notes_by_edit (deleted=?)"]
        );
    }

    /// The pile is read by the list's index, which has all it needs but
    /// the ID.
    #[test]
    fn the_pile_is_read_by_the_lists_index() {
        let (_folder, connection) = empty_database();
        assert_eq!(
            read_order(&connection, PILE_SQL, 0),
            ["SEARCH notes USING INDEX notes_by_edit (deleted=?)"]
        );
    }

    #[test]
    fn a_typed_query_becomes_quoted_prefix_terms() {
        assert_eq!(
            full_text_query("buy milk").as_deref(),
            Some("\"buy\"* \"milk\"*")
        );
    }

    #[test]
    fn punctuation_cannot_become_query_syntax() {
        assert_eq!(
            full_text_query("a\"b OR (c").as_deref(),
            Some("\"a\"\"b\"* \"OR\"* \"(c\"*")
        );
    }

    #[test]
    fn a_query_with_no_letters_or_digits_matches_nothing() {
        assert_eq!(full_text_query("  # - *  "), None);
        assert_eq!(full_text_query(""), None);
    }
}
