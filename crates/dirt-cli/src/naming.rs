//! Which note a command means: its number in the last list, its ID or the
//! start of it, words from it, or, for `dirt show` and `dirt edit`,
//! nothing at all (system design 8.5).
//!
//! The rules are tried in the order [`NAMING_A_NOTE`] gives them. A number
//! with no leading zero is always a number from the last list: a note's ID
//! begins with the time it was made, and every ID made before the year 2527
//! begins with a zero. Hexadecimal that begins no note's ID is taken as
//! words, so `dirt edit cafe` finds the note about the café.
//!
//! The last list is what `dirt list` or `dirt search` last printed, or the
//! notes a command offered to choose from. It is kept beside the database,
//! as the note IDs in the order they were shown, so that a number always
//! names the note that was shown under it, whatever has changed since.

use std::fmt::Write as _;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use chrono::Utc;
use dirt_core::store::SIGNED_OUT_MARKER;
use dirt_core::{Note, NoteId, Store};
use serde::{Deserialize, Serialize};

use crate::commands::common::{format_relative_time, listing_lines};
use crate::context::Context;
use crate::error::CliError;

/// How a note is named, for the help of every command that takes one.
pub const NAMING_A_NOTE: &str = "\
Naming a note, tried in this order:
  2          The note numbered 2 in the last `dirt list` or `dirt search`
  01a2f3c4   The note whose ID is or begins with this, as the list shows it
  groceries  The one note holding every word given; several are listed,
             numbered, to choose from
A number with no leading zero is always taken from the last list. Note IDs
begin with 0. Text that begins no note's ID is taken as words.";

/// The same, for `dirt show` and `dirt edit`, which need no note named.
pub const NAMING_A_NOTE_OR_NOT: &str = "\
Naming a note, tried in this order:
  (nothing)  The latest note: the first in `dirt list`
  2          The note numbered 2 in the last `dirt list` or `dirt search`
  01a2f3c4   The note whose ID is or begins with this, as the list shows it
  groceries  The one note holding every word given; several are listed,
             numbered, to choose from
A number with no leading zero is always taken from the last list. Note IDs
begin with 0. Text that begins no note's ID is taken as words.";

/// How many notes are offered when words or the start of an ID name more
/// than one.
pub const CHOICES: usize = 10;

/// How many characters of a note's ID a list shows at least. A note's ID
/// begins with the time it was made, to the millisecond, so notes written
/// close together share most of its start; a list shows as much more as it
/// takes to tell each from every other note.
pub const SHORT_ID: usize = 8;

/// What a command's note argument is read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteRef {
    /// Nothing: the latest note, for the commands that allow it.
    Latest,
    /// A number from the last list. Lists begin at 1.
    Number(usize),
    /// A whole ID.
    Id(NoteId),
    /// Hexadecimal digits and hyphens: the start of an ID, or else words.
    IdOrWords(String),
    /// Words from the note.
    Words(String),
}

/// Reads what was typed for a note. The words of a note may be typed
/// without quotes, so they come as several arguments and are joined.
pub fn read_note_ref(typed: &[String]) -> NoteRef {
    let text = typed.join(" ");
    let text = text.trim();
    if text.is_empty() {
        return NoteRef::Latest;
    }
    if text.bytes().all(|b| b.is_ascii_digit()) && (text == "0" || !text.starts_with('0')) {
        // More digits than any list has rows are still a number, and the
        // number is then not in the list.
        return NoteRef::Number(text.parse().unwrap_or(usize::MAX));
    }
    if let Ok(id) = text.parse::<NoteId>() {
        return NoteRef::Id(id);
    }
    if text.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return NoteRef::IdOrWords(text.to_owned());
    }
    NoteRef::Words(text.to_owned())
}

/// The note a command was told of. Nothing named is an error.
pub async fn note_named(typed: &[String], context: &Context) -> Result<Note, CliError> {
    match read_note_ref(typed) {
        NoteRef::Latest => Err(CliError::EmptyNoteId),
        reference => find(&reference, &context.store, context.can_ask).await,
    }
}

/// The note a command was told of, or the latest note if none was named.
pub async fn note_named_or_latest(typed: &[String], context: &Context) -> Result<Note, CliError> {
    find(&read_note_ref(typed), &context.store, context.can_ask).await
}

/// The note `reference` names among the notes that are not deleted.
///
/// When words or the start of an ID name several, they become the last
/// list and are shown numbered: asked about when `can_ask`, which is when
/// a person is at the terminal, and otherwise said in the error.
pub async fn find(reference: &NoteRef, store: &Store, can_ask: bool) -> Result<Note, CliError> {
    match reference {
        NoteRef::Latest => store
            .list_notes(1, 0)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                CliError::NoteNotFound(
                    "There are no notes in the list yet. `dirt \"a thought\"` writes one"
                        .to_owned(),
                )
            }),
        NoteRef::Number(number) => {
            let id = listed(store, *number)?;
            match store.get_note(&id).await? {
                Some(note) => Ok(note),
                None => Err(not_here(
                    store,
                    &id,
                    &format!("Note {number} of the last list"),
                    &number.to_string(),
                )
                .await?),
            }
        }
        NoteRef::Id(id) => match store.get_note(id).await? {
            Some(note) => Ok(note),
            None => Err(not_here(store, id, "That note", &id.to_string()).await?),
        },
        NoteRef::IdOrWords(text) => {
            let ids = store.list_note_ids_by_prefix(text, CHOICES + 1).await?;
            if ids.is_empty() {
                return by_words(text, store, can_ask).await;
            }
            let mut notes = Vec::with_capacity(ids.len());
            for id in ids {
                let id = id.parse::<NoteId>().map_err(|_| no_note(text))?;
                if let Some(note) = store.get_note(&id).await? {
                    notes.push(note);
                }
            }
            the_one_of(
                notes,
                &format!("whose IDs begin with {text}"),
                store,
                can_ask,
            )
            .await
        }
        NoteRef::Words(text) => by_words(text, store, can_ask).await,
    }
}

/// The ID of the deleted note that was named, for bringing it back: by its
/// number in the last list, which may be the list it was deleted from, by
/// its whole ID, or by the start of the ID of one deleted note this device
/// still holds. A deleted note is not found by its words.
///
/// A whole ID, or a number, is taken as it is: the store says if it holds
/// no such note, and leaves alone a note that is not deleted.
pub async fn deleted_note_named(typed: &[String], store: &Store) -> Result<NoteId, CliError> {
    match read_note_ref(typed) {
        NoteRef::Latest => Err(CliError::EmptyNoteId),
        NoteRef::Number(number) => listed(store, number),
        NoteRef::Id(id) => Ok(id),
        NoteRef::IdOrWords(prefix) => {
            let ids = store.list_deleted_note_ids_by_prefix(&prefix, 3).await?;
            match ids.as_slice() {
                [] => Err(CliError::NoteNotFound(format!(
                    "No deleted note's ID begins with {prefix}"
                ))),
                [only] => only.parse().map_err(|_| no_note(&prefix)),
                several => {
                    let ids: Vec<NoteId> =
                        several.iter().filter_map(|id| id.parse().ok()).collect();
                    let short = store.shortest_id_prefixes(&ids, true, SHORT_ID).await?;
                    Err(CliError::SeveralNotes(format!(
                        "The IDs of several deleted notes begin with {prefix}: {}. Give more of the ID",
                        short.join(", ")
                    )))
                }
            }
        }
        NoteRef::Words(_) => Err(CliError::NoteNotFound(
            "A deleted note is not found by its words. Name it by its number in the list it was \
             deleted from, or by the ID that `dirt delete` printed"
                .to_owned(),
        )),
    }
}

async fn by_words(text: &str, store: &Store, can_ask: bool) -> Result<Note, CliError> {
    let notes = store.search_notes(text, CHOICES + 1).await?;
    if notes.is_empty() {
        return Err(no_note(text));
    }
    the_one_of(notes, &format!("holding \"{text}\""), store, can_ask).await
}

fn no_note(text: &str) -> CliError {
    CliError::NoteNotFound(format!(
        "No note holds \"{text}\", and no note's ID begins with it. `dirt search` finds notes \
         by fewer words; `dirt list` shows the latest"
    ))
}

/// The one note of `notes`, or a choice among them. `which` says what they
/// share: "holding \"milk\"".
async fn the_one_of(
    mut notes: Vec<Note>,
    which: &str,
    store: &Store,
    can_ask: bool,
) -> Result<Note, CliError> {
    if notes.len() == 1 {
        return Ok(notes.remove(0));
    }
    let more = notes.len() > CHOICES;
    notes.truncate(CHOICES);
    // The choice is said where errors go.
    if read_by_a_person(&io::stderr()) {
        remember_list(store, &format!("the notes {which}"), &notes);
    }
    let mut said = format!("{} notes {which}:\n", if more { "Many" } else { "Several" });
    for line in listing_lines(&notes, store).await? {
        said.push_str(&line);
        said.push('\n');
    }
    if more {
        let _ = writeln!(
            said,
            "and more. The first {CHOICES} are shown; give more words"
        );
    }
    if can_ask {
        eprint!("{said}");
        let chosen = choose(&mut io::stdin().lock(), &mut io::stderr(), notes.len())?;
        return chosen
            .map(|number| notes.swap_remove(number - 1))
            .ok_or(CliError::NothingChosen);
    }
    said.push_str("Name one by its number: they are the last list now");
    Err(CliError::SeveralNotes(said))
}

/// Asks for a number from 1 to `count` until one is given. Nothing, or the
/// end of the input, is no choice.
pub fn choose(
    input: &mut impl BufRead,
    prompt: &mut impl Write,
    count: usize,
) -> Result<Option<usize>, CliError> {
    loop {
        write!(prompt, "Which one? Its number, or Enter for none: ")?;
        prompt.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            writeln!(prompt)?;
            return Ok(None);
        }
        let line = line.trim();
        if line.is_empty() {
            return Ok(None);
        }
        match line.parse::<usize>() {
            Ok(number) if (1..=count).contains(&number) => return Ok(Some(number)),
            _ => writeln!(prompt, "Type a number from 1 to {count}.")?,
        }
    }
}

/// Why a note that was named is not here: deleted, which can be undone,
/// or not on this device at all. `named` is how it was named in words,
/// `again` how to name it to `dirt restore`.
async fn not_here(
    store: &Store,
    id: &NoteId,
    named: &str,
    again: &str,
) -> Result<CliError, CliError> {
    let deleted = !store
        .list_deleted_note_ids_by_prefix(&id.to_string(), 1)
        .await?
        .is_empty();
    Ok(CliError::NoteNotFound(if deleted {
        format!("{named} is deleted. `dirt restore {again}` brings it back")
    } else {
        format!("{named} is not on this device")
    }))
}

// ---- The last list ----

/// What `dirt list` or `dirt search` last printed, or the notes a command
/// last offered to choose from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastList {
    /// What made it, in words: "`dirt list`".
    pub what: String,
    /// When, in Unix milliseconds.
    pub at: i64,
    /// The notes, in the order they were numbered.
    pub ids: Vec<NoteId>,
}

/// Where the last list of a store is kept: beside its database, as SQLite
/// keeps its own files, so that it goes with the store at sign-out and a
/// store named by `--db-path` has a list of its own.
pub fn list_file(store: &Store) -> PathBuf {
    let mut path = store.path().as_os_str().to_owned();
    path.push("-list");
    PathBuf::from(path)
}

/// Whether what is written to `stream` is read by a person, at a terminal,
/// and so whether a list written there is the one their numbers name. The
/// tests stand in for that person.
pub fn read_by_a_person(stream: &impl IsTerminal) -> bool {
    cfg!(test) || stream.is_terminal()
}

/// Keeps `notes` as the last list. A list that cannot be kept is said,
/// and the command still does what it was asked.
pub fn remember_list(store: &Store, what: &str, notes: &[Note]) {
    let path = list_file(store);
    // Nothing more is written to a store that is signed out of.
    if path
        .parent()
        .is_some_and(|folder| folder.join(SIGNED_OUT_MARKER).exists())
    {
        return;
    }
    let list = LastList {
        what: what.to_owned(),
        at: Utc::now().timestamp_millis(),
        ids: notes.iter().map(|note| note.id).collect(),
    };
    let written = serde_json::to_vec(&list)
        .map_err(io::Error::other)
        .and_then(|bytes| {
            // Written whole to a file of its own and then put in place, so
            // that a command run meanwhile reads the old list or the new
            // one, and two commands writing at once each put a whole list
            // in place: the last to finish is the list.
            let folder = path.parent().unwrap_or_else(|| std::path::Path::new("."));
            let mut partial = tempfile::NamedTempFile::new_in(folder)?;
            partial.write_all(&bytes)?;
            partial
                .persist(&path)
                .map(drop)
                .map_err(|error| error.error)
        });
    if let Err(error) = written {
        eprintln!("The list's numbers could not be kept for the next command: {error}");
    }
}

/// The last list, if there is one that can be read.
pub fn last_list(store: &Store) -> Option<LastList> {
    let bytes = std::fs::read(list_file(store)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The ID that `number` stood for in the last list.
fn listed(store: &Store, number: usize) -> Result<NoteId, CliError> {
    let Some(list) = last_list(store) else {
        return Err(CliError::NoteNotFound(format!(
            "There is no list to take {number} from yet. Run `dirt list` or `dirt search` \
             first, or name the note by its ID or by words from it"
        )));
    };
    if number == 0 {
        return Err(CliError::NoteNotFound(
            "A list is numbered from 1".to_owned(),
        ));
    }
    list.ids.get(number - 1).copied().ok_or_else(|| {
        let ago = format_relative_time(list.at, Utc::now().timestamp_millis());
        let count = match list.ids.len() {
            0 => "no notes".to_owned(),
            1 => "1 note".to_owned(),
            n => format!("{n} notes"),
        };
        CliError::NoteNotFound(format!(
            "The last list ({}, {ago}) had {count}: there is no {}. Run `dirt list` again",
            list.what,
            if number == usize::MAX {
                "such number".to_owned()
            } else {
                number.to_string()
            }
        ))
    })
}

/// Whether a choice can be asked: a person is at the terminal, reading
/// what goes to standard error and typing what comes in.
pub fn can_ask() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}
