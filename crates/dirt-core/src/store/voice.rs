//! Notes that were said (interface design 6.18).
//!
//! A recording is kept as a note at once: the note, its recording as a
//! file, the mark that says which file that is, and this device's row in
//! `voice_waiting`, all in one transaction. Its words are made afterwards,
//! by this device alone, and put at the start of the note in another.

use rusqlite::{params, Connection, OptionalExtension};

use super::notes::{load_for_update, save, Origin};
use super::schema::{client_id, write_transaction};
use crate::files::Attachment;
use crate::models::{Note, NoteId, VoiceMark, Words};
use crate::notes::NoteDoc;
use crate::{Error, Result};

/// A recording this device made whose words are still to be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceWaiting {
    pub note: NoteId,
    /// The recording: one of the note's files, on this device's shelf.
    pub file_id: uuid::Uuid,
    /// How many times making its words has failed.
    pub tries: u32,
}

/// What putting a recording's words in did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordsPut {
    /// The words are at the start of the note.
    Made(Note),
    /// None were found. The note keeps its recording and its name.
    NoneFound(Note),
    /// The note was deleted meanwhile, or is not a recording waiting for
    /// words any more: nothing was written, and it waits no longer.
    Nothing,
}

/// Makes the note of a recording: empty, with the recording attached and
/// marked, waiting for its words on this device.
///
/// `None`, and nothing written, when a note has this ID already, deleted or
/// not: a recording taken in twice is one note.
pub(super) fn keep(
    connection: &mut Connection,
    id: NoteId,
    attachment: &Attachment,
    duration_ms: u64,
    now_ms: i64,
) -> Result<Option<Note>> {
    let tx = write_transaction(connection)?;
    if load_for_update(&tx, &id)?.is_some() {
        return Ok(None);
    }
    let doc = NoteDoc::new(client_id(&tx)?);
    doc.attach(attachment, now_ms);
    doc.set_voice(Some(&VoiceMark {
        file_id: attachment.file_id,
        duration_ms,
        words: Words::Waiting,
    }));
    let note = save(&tx, &id, &doc, None, Origin::Local)?
        .ok_or_else(|| Error::NotFound(id.to_string()))?;
    tx.execute(
        "INSERT INTO voice_waiting (note_id, since) VALUES (?1, ?2)",
        params![id.as_str(), now_ms],
    )?;
    tx.commit()?;
    Ok(Some(note))
}

/// The recordings this device made that wait for their words, the oldest
/// first. A deleted note's recording waits, unread, for the note to come
/// back.
pub(super) fn waiting(connection: &Connection) -> Result<Vec<VoiceWaiting>> {
    let mut statement = connection.prepare(
        "SELECT w.note_id, w.tries, n.voice FROM voice_waiting w \
           JOIN notes n ON n.id = w.note_id \
          WHERE n.deleted = 0 \
          ORDER BY w.since, w.note_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, u32>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut found = Vec::new();
    for row in rows {
        let (note, tries, voice) = row?;
        let Ok(note) = note.parse::<NoteId>() else {
            continue;
        };
        let Some(mark) = voice.as_deref().and_then(VoiceMark::from_entry) else {
            continue;
        };
        if mark.words == Words::Waiting {
            found.push(VoiceWaiting {
                note,
                file_id: mark.file_id,
                tries,
            });
        }
    }
    Ok(found)
}

/// Why the words of each of these notes could not be made, for those whose
/// tries have run out ([`crate::voice::MOST_TRIES`]).
pub(super) fn problems(
    connection: &Connection,
    notes: &[NoteId],
    most_tries: u32,
) -> Result<std::collections::HashMap<NoteId, String>> {
    let mut found = std::collections::HashMap::new();
    let mut statement = connection
        .prepare("SELECT problem FROM voice_waiting WHERE note_id = ?1 AND tries >= ?2")?;
    for note in notes {
        let problem: Option<Option<String>> = statement
            .query_row(params![note.as_str(), most_tries], |row| row.get(0))
            .optional()?;
        if let Some(problem) = problem {
            found.insert(*note, problem.unwrap_or_default());
        }
    }
    Ok(found)
}

/// Whether this device has a recording of this note waiting for words.
pub(super) fn is_waiting(connection: &Connection, id: &NoteId) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM voice_waiting WHERE note_id = ?1",
            [id.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Makes this device the one that makes the note's words: a note moved
/// here from the store it was recorded in.
pub(super) fn wait(connection: &mut Connection, id: &NoteId, now_ms: i64) -> Result<()> {
    let tx = write_transaction(connection)?;
    if load_for_update(&tx, id)?.is_some() {
        tx.execute(
            "INSERT OR IGNORE INTO voice_waiting (note_id, since) VALUES (?1, ?2)",
            params![id.as_str(), now_ms],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Puts a recording's words at the start of its note, and marks them made.
/// The note waits no longer.
///
/// Words already on the note, typed while it waited, follow after a blank
/// line. With `text_only`, the recording is then taken off the note, and
/// the mark with it: the note is a note like any other. With no words the
/// note keeps its recording whatever was chosen, so that nothing said is
/// lost, and only its mark changes: that is not an edit.
pub(super) fn put_words(
    connection: &mut Connection,
    id: &NoteId,
    words: &str,
    text_only: bool,
    now_ms: i64,
) -> Result<WordsPut> {
    let tx = write_transaction(connection)?;
    let done = |tx: &rusqlite::Transaction<'_>| {
        tx.execute(
            "DELETE FROM voice_waiting WHERE note_id = ?1",
            [id.as_str()],
        )
    };
    let Some((row, bytes, _)) = load_for_update(&tx, id)? else {
        tx.commit()?;
        return Ok(WordsPut::Nothing);
    };
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    let mark = doc.voice().filter(|mark| mark.words == Words::Waiting);
    let (Some(mark), false) = (mark, doc.is_deleted()) else {
        done(&tx)?;
        tx.commit()?;
        return Ok(WordsPut::Nothing);
    };
    let words = words.trim();
    let put = if words.is_empty() {
        doc.set_voice(Some(&VoiceMark {
            words: Words::NoneFound,
            ..mark
        }));
        let note = save(&tx, id, &doc, Some(row), Origin::Mark)?
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        WordsPut::NoneFound(note)
    } else {
        let typed = doc.text();
        let text = if typed.trim().is_empty() {
            words.to_owned()
        } else {
            format!("{words}\n\n{typed}")
        };
        doc.set_text(&text, now_ms);
        if text_only {
            doc.set_voice(None);
            doc.detach(mark.file_id, now_ms);
        } else {
            doc.set_voice(Some(&VoiceMark {
                words: Words::Made,
                ..mark
            }));
        }
        let note = save(&tx, id, &doc, Some(row), Origin::Local)?
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        WordsPut::Made(note)
    };
    done(&tx)?;
    tx.commit()?;
    Ok(put)
}

/// What a try says when nothing came back from it: the process ended
/// while it read (Android stops an app that runs out of memory).
pub const READING_STOPPED: &str = "the reading stopped before it ended";

/// A try at the note's words begins: it is counted now, before the model
/// is loaded, so that a reading the process does not survive is counted
/// too, and says so until another reason replaces it.
pub(super) fn reading(connection: &mut Connection, id: &NoteId) -> Result<()> {
    let tx = write_transaction(connection)?;
    tx.execute(
        "UPDATE voice_waiting SET tries = tries + 1, problem = ?2 WHERE note_id = ?1",
        params![id.as_str(), READING_STOPPED],
    )?;
    tx.commit()?;
    Ok(())
}

/// Making the note's words failed: it waits on, and says why. The try was
/// counted as it began ([`reading`]).
pub(super) fn not_read(connection: &mut Connection, id: &NoteId, why: &str) -> Result<()> {
    let tx = write_transaction(connection)?;
    tx.execute(
        "UPDATE voice_waiting SET problem = ?2 WHERE note_id = ?1",
        params![id.as_str(), why],
    )?;
    tx.commit()?;
    Ok(())
}
