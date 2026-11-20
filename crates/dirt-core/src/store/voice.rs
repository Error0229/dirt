//! Notes that were said (interface design 6.18).
//!
//! A recording is kept as a note at once: the note, its recording as a
//! file, the mark that says which file that is, and this device's row in
//! `voice_waiting`, all in one transaction. Its words are made afterwards,
//! by this device alone, and put at the start of the note in another.
//!
//! A recording said into a note being written (🎙 in Android's page, 8.9)
//! is added to that note instead: one more of its files, unmarked, and
//! this device's row in `voice_added`. Its words go at the note's end.

use dirt_proto::api::limits;
use rusqlite::{params, Connection, OptionalExtension};

use super::notes::{get, load_for_update, save, Origin};
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
    /// The recording was added to a note that was there already
    /// ([`crate::Store::add_voice_to_note`]): its words go at the end of
    /// the note, which is not named by it. Otherwise the note is the
    /// recording's own, and its words go at the start.
    pub added: bool,
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
    let mut found: Vec<(i64, VoiceWaiting)> = Vec::new();
    let mut statement = connection.prepare(
        "SELECT w.note_id, w.tries, n.voice, w.since FROM voice_waiting w \
           JOIN notes n ON n.id = w.note_id \
          WHERE n.deleted = 0",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, u32>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (note, tries, voice, since) = row?;
        let Ok(note) = note.parse::<NoteId>() else {
            continue;
        };
        let Some(mark) = voice.as_deref().and_then(VoiceMark::from_entry) else {
            continue;
        };
        if mark.words == Words::Waiting {
            let waiting = VoiceWaiting {
                note,
                file_id: mark.file_id,
                tries,
                added: false,
            };
            found.push((since, waiting));
        }
    }
    // A recording added to a note waits while the note still has it: one
    // taken off the note, here or on another device, is not read.
    let mut statement = connection.prepare(
        "SELECT a.note_id, a.file_id, a.tries, a.since FROM voice_added a \
           JOIN notes n ON n.id = a.note_id \
           JOIN note_files f ON f.note_id = a.note_id AND f.file_id = a.file_id \
          WHERE n.deleted = 0",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, u32>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (note, file_id, tries, since) = row?;
        let (Ok(note), Ok(file_id)) = (note.parse::<NoteId>(), file_id.parse::<uuid::Uuid>())
        else {
            continue;
        };
        let waiting = VoiceWaiting {
            note,
            file_id,
            tries,
            added: true,
        };
        found.push((since, waiting));
    }
    // The oldest first, whichever kind.
    found.sort_by(|(at, one), (other_at, other)| {
        (at, one.note, one.file_id).cmp(&(other_at, other.note, other.file_id))
    });
    Ok(found.into_iter().map(|(_, waiting)| waiting).collect())
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
    // A recording added to the note whose words could not be made says so
    // on the note's row too, while the note still has it.
    let mut added = connection.prepare(
        "SELECT a.problem FROM voice_added a \
           JOIN note_files f ON f.note_id = a.note_id AND f.file_id = a.file_id \
          WHERE a.note_id = ?1 AND a.tries >= ?2 ORDER BY a.since LIMIT 1",
    )?;
    for note in notes {
        let own: Option<Option<String>> = statement
            .query_row(params![note.as_str(), most_tries], |row| row.get(0))
            .optional()?;
        let problem = match own {
            Some(problem) => Some(problem),
            None => added
                .query_row(params![note.as_str(), most_tries], |row| row.get(0))
                .optional()?,
        };
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

/// The recordings added to this note on this device whose words are still
/// to be made, each with when it was added.
pub(super) fn added_waiting(
    connection: &Connection,
    id: &NoteId,
) -> Result<Vec<(uuid::Uuid, i64)>> {
    let mut statement =
        connection.prepare("SELECT file_id, since FROM voice_added WHERE note_id = ?1")?;
    let rows = statement.query_map([id.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut found = Vec::new();
    for row in rows {
        let (file_id, since) = row?;
        if let Ok(file_id) = file_id.parse() {
            found.push((file_id, since));
        }
    }
    Ok(found)
}

/// Makes this device the one that makes the words of the recordings added
/// to the note: a note moved here from the store they were said into.
pub(super) fn wait_added(
    connection: &mut Connection,
    id: &NoteId,
    files: &[(uuid::Uuid, i64)],
) -> Result<()> {
    let tx = write_transaction(connection)?;
    if load_for_update(&tx, id)?.is_some() {
        for (file_id, since) in files {
            tx.execute(
                "INSERT OR IGNORE INTO voice_added (file_id, note_id, since) VALUES (?1, ?2, ?3)",
                params![file_id.to_string(), id.as_str(), since],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Adds a recording to a note that is there: one more of its files, and
/// this device's row in `voice_added`, in one transaction. The note is not
/// marked: it is a note being written, not one said, and is not named by
/// the recording.
///
/// A note that is not here, or is deleted, is [`Error::NotFound`]; one with
/// no room for another file is [`Error::InvalidInput`], as attaching any
/// file is.
pub(super) fn add(
    connection: &mut Connection,
    id: &NoteId,
    attachment: &Attachment,
    now_ms: i64,
) -> Result<Note> {
    let tx = write_transaction(connection)?;
    let not_found = || Error::NotFound(id.to_string());
    let (row, bytes, _) = load_for_update(&tx, id)?.ok_or_else(not_found)?;
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() {
        return Err(not_found());
    }
    let most = usize::try_from(limits::MAX_FILES_PER_NOTE).unwrap_or(usize::MAX);
    if doc.named_files().len() >= most {
        return Err(Error::InvalidInput(format!(
            "a note may have at most {most} files, and a picture's preview counts as one"
        )));
    }
    if !doc.attach(attachment, now_ms) {
        return Err(Error::InvalidInput(
            "the note already has this file".to_owned(),
        ));
    }
    let note = save(&tx, id, &doc, Some(row), Origin::Local)?.ok_or_else(not_found)?;
    tx.execute(
        "INSERT INTO voice_added (file_id, note_id, since) VALUES (?1, ?2, ?3)",
        params![attachment.file_id.to_string(), id.as_str(), now_ms],
    )?;
    tx.commit()?;
    Ok(note)
}

/// Where the words of a recording added to a note go: at the end of what
/// the note says, after a blank line.
fn at_the_end(typed: &str, words: &str) -> String {
    if typed.trim().is_empty() {
        words.to_owned()
    } else if typed.ends_with("\n\n") {
        format!("{typed}{words}")
    } else if typed.ends_with('\n') {
        format!("{typed}\n{words}")
    } else {
        format!("{typed}\n\n{words}")
    }
}

/// Puts the words of a recording added to a note at the note's end, after
/// a blank line, and marks them made: it waits no longer.
///
/// With `text_only` the recording is then taken off the note. With no
/// words the note is left as it is, recording and all, so that nothing
/// said is lost. A note deleted meanwhile, or one that no longer has the
/// recording, is not written: the person took the recording off.
pub(super) fn put_added_words(
    connection: &mut Connection,
    id: &NoteId,
    file_id: uuid::Uuid,
    words: &str,
    text_only: bool,
    now_ms: i64,
) -> Result<WordsPut> {
    let tx = write_transaction(connection)?;
    tx.execute(
        "DELETE FROM voice_added WHERE file_id = ?1",
        [file_id.to_string()],
    )?;
    let Some((row, bytes, _)) = load_for_update(&tx, id)? else {
        tx.commit()?;
        return Ok(WordsPut::Nothing);
    };
    let doc = NoteDoc::from_bytes(client_id(&tx)?, &bytes)?;
    if doc.is_deleted() || !doc.named_files().contains(&file_id) {
        tx.commit()?;
        return Ok(WordsPut::Nothing);
    }
    let words = words.trim();
    if words.is_empty() {
        let note = get(&tx, id)?.ok_or_else(|| Error::NotFound(id.to_string()))?;
        tx.commit()?;
        return Ok(WordsPut::NoneFound(note));
    }
    doc.set_text(&at_the_end(&doc.text(), words), now_ms);
    if text_only {
        doc.detach(file_id, now_ms);
    }
    let note = save(&tx, id, &doc, Some(row), Origin::Local)?
        .ok_or_else(|| Error::NotFound(id.to_string()))?;
    tx.commit()?;
    Ok(WordsPut::Made(note))
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
pub(super) fn reading(connection: &mut Connection, waiting: &VoiceWaiting) -> Result<()> {
    let tx = write_transaction(connection)?;
    if waiting.added {
        tx.execute(
            "UPDATE voice_added SET tries = tries + 1, problem = ?2 WHERE file_id = ?1",
            params![waiting.file_id.to_string(), READING_STOPPED],
        )?;
    } else {
        tx.execute(
            "UPDATE voice_waiting SET tries = tries + 1, problem = ?2 WHERE note_id = ?1",
            params![waiting.note.as_str(), READING_STOPPED],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Making the words failed: the recording waits on, and says why. The try
/// was counted as it began ([`reading`]).
pub(super) fn not_read(
    connection: &mut Connection,
    waiting: &VoiceWaiting,
    why: &str,
) -> Result<()> {
    let tx = write_transaction(connection)?;
    if waiting.added {
        tx.execute(
            "UPDATE voice_added SET problem = ?2 WHERE file_id = ?1",
            params![waiting.file_id.to_string(), why],
        )?;
    } else {
        tx.execute(
            "UPDATE voice_waiting SET problem = ?2 WHERE note_id = ?1",
            params![waiting.note.as_str(), why],
        )?;
    }
    tx.commit()?;
    Ok(())
}
