//! Reading and writing notes.
//!
//! A note's row holds its CRDT document and four things derived from it: the
//! text, the last-edited time, whether it is deleted, and (in two other
//! tables) its tags and its entry in the full-text index. Every write loads
//! the document, changes it and saves it with its derived values inside one
//! `BEGIN IMMEDIATE` transaction, so the derived values can never disagree
//! with the document, and two processes can never interleave on one note.

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};

use dirt_proto::api::limits;

use super::schema::{client_id, write_transaction};
use crate::files::Attachment;
use crate::models::{extract_tags, Note, NoteId};
use crate::notes::NoteDoc;
use crate::{Error, Result};

const NOTE_COLUMNS: &str = "id, text, edited_at";

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
    })
}

/// Where a document being saved came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    /// An edit made on this device. It raises the note's revision, which
    /// marks the note as unsent.
    Local,
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
    // A document always has an edit time once it has been edited. One that
    // has not, which only a merge of an empty document could produce, falls
    // back to the creation time.
    let edited_at = doc
        .edited_at_ms()
        .or_else(|| id.created_at_ms())
        .unwrap_or_default();
    let bytes = doc.encode();

    let local = origin == Origin::Local;
    // When this device first saw the note as deleted: kept while it stays
    // deleted, and forgotten if it comes back.
    let now = crate::util::now_ms();
    let row = if let Some(row) = existing_row {
        tx.execute(
            "UPDATE notes SET doc = ?1, text = ?2, edited_at = ?3, deleted = ?4, \
                    revision = revision + ?6, \
                    deleted_at = CASE WHEN ?4 THEN COALESCE(deleted_at, ?7) END \
              WHERE rowid = ?5",
            params![bytes, text, edited_at, deleted, row, i64::from(local), now],
        )?;
        row
    } else {
        // A note that arrives from the server starts out as sent.
        let pushed_revision = if local { None } else { Some(1) };
        let deleted_at = deleted.then_some(now);
        tx.execute(
            "INSERT INTO notes (id, doc, text, edited_at, deleted, revision, pushed_revision, \
                                deleted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7)",
            params![
                key,
                bytes,
                text,
                edited_at,
                deleted,
                pushed_revision,
                deleted_at
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
    let tx = write_transaction(connection)?;
    if load(&tx, &id)?.is_some() {
        return Err(Error::InvalidInput(format!("note {id} already exists")));
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
    Ok(note)
}

pub(super) fn update(
    connection: &mut Connection,
    id: &NoteId,
    text: &str,
    now_ms: i64,
) -> Result<Note> {
    let tx = write_transaction(connection)?;
    let (row, bytes) = load(&tx, id)?.ok_or_else(|| not_found(id))?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Err(not_found(id));
    }
    if !doc.set_text(text, now_ms) {
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
pub(super) fn attach(
    connection: &mut Connection,
    id: &NoteId,
    attachment: &Attachment,
    now_ms: i64,
) -> Result<()> {
    let tx = write_transaction(connection)?;
    let (row, bytes) = load(&tx, id)?.ok_or_else(|| not_found(id))?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Err(not_found(id));
    }
    let most = usize::try_from(limits::MAX_FILES_PER_NOTE).unwrap_or(usize::MAX);
    if doc.attachments().len() >= most {
        return Err(Error::InvalidInput(format!(
            "a note may have at most {most} files"
        )));
    }
    if !doc.attach(attachment, now_ms) {
        return Err(Error::InvalidInput(
            "the note already has this file".to_owned(),
        ));
    }
    save(&tx, id, &doc, Some(row), Origin::Local)?;
    tx.commit()?;
    Ok(())
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
    doc.restore(now_ms);
    let note = save(&tx, id, &doc, Some(row), Origin::Local)?.ok_or_else(|| not_found(id))?;
    tx.commit()?;
    Ok(note)
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

pub(super) fn get(connection: &Connection, id: &NoteId) -> Result<Option<Note>> {
    Ok(connection
        .query_row(
            &format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?1 AND deleted = 0"),
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

/// Live notes, most recently edited first.
pub(super) fn list(connection: &Connection, limit: usize, offset: usize) -> Result<Vec<Note>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {NOTE_COLUMNS} FROM notes WHERE deleted = 0 \
          ORDER BY edited_at DESC, id DESC LIMIT ?1 OFFSET ?2"
    ))?;
    let rows = statement.query_map(
        params![to_sql_count(limit), to_sql_count(offset)],
        note_from_row,
    )?;
    collect(rows)
}

pub(super) fn list_by_tag(
    connection: &Connection,
    tag: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Note>> {
    let mut statement = connection.prepare(
        "SELECT n.id, n.text, n.edited_at FROM notes n \
           JOIN note_tags t ON t.note_id = n.id \
          WHERE t.tag = ?1 AND n.deleted = 0 \
          ORDER BY n.edited_at DESC, n.id DESC LIMIT ?2 OFFSET ?3",
    )?;
    let tag = tag.trim().trim_start_matches('#').to_lowercase();
    let rows = statement.query_map(
        params![tag, to_sql_count(limit), to_sql_count(offset)],
        note_from_row,
    )?;
    collect(rows)
}

/// Every tag in use, with the number of live notes that carry it.
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

/// The IDs of live notes whose ID starts with `prefix`, for commands that
/// accept a shortened ID.
pub(super) fn ids_by_prefix(
    connection: &Connection,
    prefix: &str,
    limit: usize,
) -> Result<Vec<String>> {
    // IDs are hexadecimal digits and hyphens. Anything else cannot match, and
    // refusing it keeps `%` and `_` out of the LIKE pattern.
    let prefix = prefix.trim().to_lowercase();
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare("SELECT id FROM notes WHERE deleted = 0 AND id LIKE ?1 ORDER BY id LIMIT ?2")?;
    let rows = statement.query_map(params![format!("{prefix}%"), to_sql_count(limit)], |row| {
        row.get::<_, String>(0)
    })?;
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
fn full_text_query(input: &str) -> Option<String> {
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

/// Notes matching a full-text query, best match first.
///
/// `CROSS JOIN` is SQLite's way of fixing the order of a join: the table
/// on its left is read first. Left to choose, SQLite reads `notes` first
/// and asks the index about each note in turn, and each such question
/// walks the whole list of notes that hold the word. With 20,000 notes
/// holding the word typed, that took 22 seconds. Read first, the index
/// answers once.
const SEARCH_SQL: &str = "SELECT n.id, n.text, n.edited_at FROM notes_fts f \
       CROSS JOIN notes n ON n.rowid = f.rowid \
      WHERE notes_fts MATCH ?1 AND n.deleted = 0 \
      ORDER BY f.rank, n.edited_at DESC LIMIT ?2";

/// The same, for notes that also carry a tag. The index is read first here
/// too, then the note, then its tags.
const SEARCH_WITH_TAG_SQL: &str = "SELECT n.id, n.text, n.edited_at FROM notes_fts f \
       CROSS JOIN notes n ON n.rowid = f.rowid \
       CROSS JOIN note_tags t ON t.note_id = n.id AND t.tag = ?2 \
      WHERE notes_fts MATCH ?1 AND n.deleted = 0 \
      ORDER BY f.rank, n.edited_at DESC LIMIT ?3";

/// Live notes matching every word of `query`, best match first.
pub(super) fn search(connection: &Connection, query: &str, limit: usize) -> Result<Vec<Note>> {
    let Some(query) = full_text_query(query) else {
        return Ok(Vec::new());
    };
    let mut statement = connection.prepare(SEARCH_SQL)?;
    let rows = statement.query_map(params![query, to_sql_count(limit)], note_from_row)?;
    collect(rows)
}

/// Live notes that match every word of `query` and carry `tag`. Either may
/// be left out: an empty query matches every note, and no tag means any.
///
/// With words to match, the best match comes first; without, the most
/// recently edited. A query with text but no letter or digit matches
/// nothing, as it does in [`search`].
pub(super) fn find(
    connection: &Connection,
    query: &str,
    tag: Option<&str>,
    limit: usize,
) -> Result<Vec<Note>> {
    let tag = tag
        .map(|tag| tag.trim().trim_start_matches('#').to_lowercase())
        .filter(|tag| !tag.is_empty());
    let Some(words) = full_text_query(query) else {
        if !query.trim().is_empty() {
            return Ok(Vec::new());
        }
        return tag.map_or_else(
            || list(connection, limit, 0),
            |tag| list_by_tag(connection, &tag, limit, 0),
        );
    };
    let Some(tag) = tag else {
        return search(connection, query, limit);
    };
    let mut statement = connection.prepare(SEARCH_WITH_TAG_SQL)?;
    let rows = statement.query_map(params![words, tag, to_sql_count(limit)], note_from_row)?;
    collect(rows)
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
    fn read_order(connection: &Connection, sql: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        let steps = statement
            .query_map(params!["\"word\"*", "tag", 100], |row| {
                row.get::<_, String>(3)
            })
            .unwrap();
        steps
            .map(|step| step.expect("a step of the plan"))
            .filter(|step| step.starts_with("SCAN") || step.starts_with("SEARCH"))
            .collect()
    }

    /// A search has to begin at the full-text index. Begun at the notes,
    /// it asks the index once for every note, which with many notes takes
    /// many seconds.
    #[test]
    fn a_search_reads_the_full_text_index_first() {
        let folder = tempfile::tempdir().unwrap();
        let mut connection =
            super::super::schema::open_connection(&folder.path().join("notes.db")).unwrap();
        super::super::schema::set_up(&mut connection).unwrap();

        let plain = read_order(&connection, &SEARCH_SQL.replace("?2", "?3"));
        assert!(
            plain[0].contains("SCAN f VIRTUAL TABLE"),
            "the search reads in this order: {plain:?}"
        );
        assert!(plain[1].starts_with("SEARCH n "), "{plain:?}");

        let tagged = read_order(&connection, SEARCH_WITH_TAG_SQL);
        assert!(
            tagged[0].contains("SCAN f VIRTUAL TABLE"),
            "the search with a tag reads in this order: {tagged:?}"
        );
        assert!(tagged[1].starts_with("SEARCH n "), "{tagged:?}");
        assert!(tagged[2].starts_with("SEARCH t "), "{tagged:?}");
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
