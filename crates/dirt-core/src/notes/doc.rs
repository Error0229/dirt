//! The note document: one yrs document per note.
//!
//! Layout inside the yrs document:
//!
//! - `body`: the note text.
//! - `edits`: a map from device (client ID) to `"<count>:<edited_at_ms>"`.
//!   `count` is how many edits that device has made. Together the counts form
//!   a version vector for the note's content.
//! - `deletions`: a map from device to the version vector that device had seen
//!   when it deleted the note.
//! - `files`: a map from a file's ID to the file as this note names it: its
//!   name, type, size and key (`crate::files::Attachment`). A file never
//!   changes, so an entry is written once and at most removed. Two devices
//!   that each attach a file write two entries, and both are kept.
//! - `marks`: single values that say something about the note and are not
//!   part of what it holds. There is one, `archived`: `true` while the note
//!   is archived. A document written before archive has no such map and
//!   reads as not archived.
//!
//! Attaching a file and removing one are edits, like a change to the text:
//! each raises the device's count. A device that deleted the note without
//! having seen a file attached has not deleted that, and the note comes
//! back with the file.
//!
//! Setting a mark is not an edit. It raises no count and records no time,
//! so archiving a note changes neither its edit time nor whether it is
//! deleted: a note that another device deleted stays deleted when this one
//! archives it. When two devices set a mark differently without having seen
//! each other's choice, yrs keeps one of the two, the same one on every
//! device.
//!
//! A version of Dirt from before a map was added never asks yrs for it, and
//! keeps it all the same: yrs stores what it merges under the name it came
//! with, whether or not this program has asked for that name, and writes it
//! out again. `tests/notes_archive.rs` shows it for `marks`.
//!
//! The note counts as deleted while at least one deletion marker covers the
//! whole current version vector. An edit the deleter had not seen raises a
//! count above the marker, so the note comes back with that edit.
//!
//! The version vector is kept in the document on purpose. The yrs state vector
//! looks like the natural thing to compare, but it does not move when an edit
//! only removes characters, and writing a marker moves the deleter's own entry.
//! `tests/notes_deletion.rs` demonstrates the first problem.

use std::collections::BTreeMap;

use yrs::updates::decoder::Decode;
use yrs::{
    Any, ClientID, Doc, GetString, Map, MapRef, Options, Out, ReadTxn, StateVector, Text, TextRef,
    Transact, TransactionMut, Update,
};

use super::diff::{diff, Op};
use crate::files::Attachment;

/// Edit counts per device.
pub type VersionVector = BTreeMap<u64, u64>;

/// Errors from loading or merging a document.
#[derive(Debug, thiserror::Error)]
pub enum DocError {
    /// The bytes are not a valid yrs update.
    #[error("cannot decode note document: {0}")]
    Decode(String),
    /// The update could not be applied.
    #[error("cannot apply note document: {0}")]
    Apply(String),
}

/// One note, as a CRDT document owned by one device.
pub struct NoteDoc {
    doc: Doc,
    body: TextRef,
    edits: MapRef,
    deletions: MapRef,
    files: MapRef,
    marks: MapRef,
    client_id: u64,
}

impl NoteDoc {
    /// Creates an empty note for the device with this client ID.
    ///
    /// The ID must be unique per install and below 2^53.
    #[must_use]
    pub fn new(client_id: u64) -> Self {
        let doc = Doc::with_options(Options::with_client_id(ClientID::new(client_id)));
        // Root types must be fetched before any transaction is opened.
        let body = doc.get_or_insert_text("body");
        let edits = doc.get_or_insert_map("edits");
        let deletions = doc.get_or_insert_map("deletions");
        let files = doc.get_or_insert_map("files");
        let marks = doc.get_or_insert_map("marks");
        Self {
            doc,
            body,
            edits,
            deletions,
            files,
            marks,
            client_id,
        }
    }

    /// Loads a note from its encoded state, to be edited by this device.
    pub fn from_bytes(client_id: u64, bytes: &[u8]) -> Result<Self, DocError> {
        let note = Self::new(client_id);
        note.merge(bytes)?;
        Ok(note)
    }

    /// The client ID this document writes under.
    #[must_use]
    pub const fn client_id(&self) -> u64 {
        self.client_id
    }

    /// The current note text.
    #[must_use]
    pub fn text(&self) -> String {
        self.body.get_string(&self.doc.transact())
    }

    /// Replaces the note text, applying only the spans that changed.
    ///
    /// Returns `false`, and writes nothing, when the text is unchanged.
    pub fn set_text(&self, new: &str, now_ms: i64) -> bool {
        let old = self.text();
        if old == new {
            return false;
        }
        let ops = diff(&old, new);
        let mut txn = self.doc.transact_mut();
        let mut pos = 0u32;
        for op in ops {
            match op {
                Op::Keep(n) => pos += n,
                Op::Delete(n) => self.body.remove_range(&mut txn, pos, n),
                Op::Insert(s) => {
                    self.body.insert(&mut txn, pos, &s);
                    pos += u32::try_from(s.len()).expect("note text fits in u32");
                }
            }
        }
        self.record_edit(&mut txn, now_ms);
        true
    }

    /// The files attached to the note, in the order they were attached.
    ///
    /// An entry that this version cannot read is left out. It stays in the
    /// document, for a version that can.
    #[must_use]
    pub fn attachments(&self) -> Vec<Attachment> {
        let txn = self.doc.transact();
        let mut attachments: Vec<Attachment> = self
            .files
            .iter(&txn)
            .filter_map(|(file_id, value)| Attachment::from_entry(file_id, out_as_str(&value)?))
            .collect();
        attachments.sort_by_key(|a| (a.added_at_ms, a.file_id));
        attachments
    }

    /// The ID of every file the note names: each attachment, and each
    /// thumbnail of one.
    ///
    /// An entry that this version cannot read is counted too, by its ID.
    /// A later version wrote it and still wants the file. Were it left
    /// out, this device would in time tell the server that the note had
    /// stopped naming the file.
    #[must_use]
    pub fn named_files(&self) -> Vec<uuid::Uuid> {
        let txn = self.doc.transact();
        let mut files = Vec::new();
        for (file_id, value) in self.files.iter(&txn) {
            let Ok(id) = file_id.parse() else { continue };
            files.push(id);
            let thumbnail = out_as_str(&value)
                .and_then(|entry| Attachment::from_entry(file_id, entry))
                .and_then(|attachment| attachment.thumbnail);
            if let Some(thumbnail) = thumbnail {
                files.push(thumbnail.file_id);
            }
        }
        files
    }

    /// Attaches a file to the note. This is an edit.
    ///
    /// Returns `false`, and writes nothing, if the note already names a
    /// file with this ID: a file never changes.
    pub fn attach(&self, attachment: &Attachment, now_ms: i64) -> bool {
        let key = attachment.file_id.to_string();
        let mut txn = self.doc.transact_mut();
        if self.files.get(&txn, &key).is_some() {
            return false;
        }
        self.files.insert(&mut txn, key, attachment.to_entry());
        self.record_edit(&mut txn, now_ms);
        true
    }

    /// Removes a file from the note. This is an edit.
    ///
    /// Returns `false`, and writes nothing, if the note names no such file.
    pub fn detach(&self, file_id: uuid::Uuid, now_ms: i64) -> bool {
        let mut txn = self.doc.transact_mut();
        if self.files.remove(&mut txn, &file_id.to_string()).is_none() {
            return false;
        }
        self.record_edit(&mut txn, now_ms);
        true
    }

    /// Deletes the note as this device sees it now.
    ///
    /// The marker records the content version this device has seen. It does
    /// not count as an edit.
    pub fn delete(&self) {
        let mut txn = self.doc.transact_mut();
        let seen = encode_vector(&read_vector(&self.edits, &txn));
        self.deletions
            .insert(&mut txn, self.client_id.to_string(), seen);
    }

    /// Brings a deleted note back. This is an ordinary edit, made at
    /// `now_ms`.
    pub fn restore(&self, now_ms: i64) {
        let mut txn = self.doc.transact_mut();
        self.record_edit(&mut txn, now_ms);
    }

    /// Brings a deleted note back where it was: what undoing a delete does.
    ///
    /// It is an edit, as [`Self::restore`] is: this device's count rises
    /// above every deletion marker, so the note comes back on every device
    /// that merges it, and the change is sent like any other. What differs
    /// is the time recorded with it. That is the note's edit time as it
    /// was, not the time now, so the note keeps its place among the others
    /// and its layer (interface design, 6.19). The edit time is the latest
    /// of the times recorded, and this one is no later than that, so it
    /// does not move. A note with no edit time yet is given `now_ms`.
    ///
    /// The time decides nothing in a merge: it is only shown, and orders
    /// the list. What brings the note back is the count.
    pub fn bring_back(&self, now_ms: i64) {
        let at = self.edited_at_ms().unwrap_or(now_ms);
        let mut txn = self.doc.transact_mut();
        self.record_edit(&mut txn, at);
    }

    /// Archives the note, or brings it back to the list.
    ///
    /// This is not an edit: the note keeps its edit time, and a note that
    /// another device has deleted stays deleted.
    ///
    /// Returns `false`, and writes nothing, when the note is already as
    /// asked.
    pub fn set_archived(&self, archived: bool) -> bool {
        let mut txn = self.doc.transact_mut();
        if read_archived(&self.marks, &txn) == archived {
            return false;
        }
        self.marks.insert(&mut txn, ARCHIVED, archived);
        true
    }

    /// Whether the note is archived: out of the list, and still a note.
    #[must_use]
    pub fn is_archived(&self) -> bool {
        read_archived(&self.marks, &self.doc.transact())
    }

    /// Whether the note is deleted: some marker covers every edit made so far.
    #[must_use]
    pub fn is_deleted(&self) -> bool {
        let txn = self.doc.transact();
        let current = read_vector(&self.edits, &txn);
        self.deletions.iter(&txn).any(|(_, value)| {
            out_as_str(&value)
                .map(decode_vector)
                .is_some_and(|marker| covers(&marker, &current))
        })
    }

    /// The time of the most recent edit by any device, for display.
    #[must_use]
    pub fn edited_at_ms(&self) -> Option<i64> {
        let txn = self.doc.transact();
        self.edits
            .iter(&txn)
            .filter_map(|(_, value)| out_as_str(&value).and_then(parse_edit_entry))
            .map(|(_, at)| at)
            .max()
    }

    /// The note's content version: edit counts per device.
    #[must_use]
    pub fn version_vector(&self) -> VersionVector {
        read_vector(&self.edits, &self.doc.transact())
    }

    /// Merges another copy's encoded state into this one.
    ///
    /// Merging is commutative and idempotent: the order does not matter, and
    /// merging the same state twice changes nothing.
    pub fn merge(&self, bytes: &[u8]) -> Result<(), DocError> {
        let update = Update::decode_v2(bytes).map_err(|e| DocError::Decode(e.to_string()))?;
        self.doc
            .transact_mut()
            .apply_update(update)
            .map_err(|e| DocError::Apply(e.to_string()))
    }

    /// The whole document in the compact (v2) encoding. This is what is
    /// stored, and from step 5 encrypted and uploaded.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v2(&StateVector::default())
    }

    /// Whether `incoming` holds more of this device's own writes than this
    /// device does.
    ///
    /// That can only happen when the device has lost local state, for example
    /// after restoring an old backup. Writing more under the same client ID
    /// would then reuse sequence numbers, so the caller must switch to a new
    /// client ID first.
    pub fn incoming_knows_more_of_me(&self, incoming: &[u8]) -> Result<bool, DocError> {
        let update = Update::decode_v2(incoming).map_err(|e| DocError::Decode(e.to_string()))?;
        let theirs = update.state_vector();
        let mine = self.doc.transact().state_vector();
        let me = ClientID::new(self.client_id);
        Ok(theirs.get(&me) > mine.get(&me))
    }

    /// Bumps this device's edit count and records the time.
    fn record_edit(&self, txn: &mut TransactionMut<'_>, now_ms: i64) {
        let key = self.client_id.to_string();
        let count = self
            .edits
            .get(txn, &key)
            .and_then(|value| out_as_str(&value).and_then(parse_edit_entry))
            .map_or(0, |(count, _)| count);
        self.edits
            .insert(txn, key, format!("{}:{now_ms}", count + 1));
    }
}

/// The key of the archive mark in `marks`.
const ARCHIVED: &str = "archived";

/// A mark that is missing, or is anything but `true`, reads as not archived.
fn read_archived<T: ReadTxn>(marks: &MapRef, txn: &T) -> bool {
    matches!(marks.get(txn, ARCHIVED), Some(Out::Any(Any::Bool(true))))
}

/// Whether `marker` has seen every edit in `current`.
fn covers(marker: &VersionVector, current: &VersionVector) -> bool {
    current
        .iter()
        .all(|(device, count)| marker.get(device).copied().unwrap_or(0) >= *count)
}

fn read_vector<T: ReadTxn>(edits: &MapRef, txn: &T) -> VersionVector {
    edits
        .iter(txn)
        .filter_map(|(device, value)| {
            let device = device.parse::<u64>().ok()?;
            let (count, _) = out_as_str(&value).and_then(parse_edit_entry)?;
            Some((device, count))
        })
        .collect()
}

/// Parses `"<count>:<edited_at_ms>"`.
fn parse_edit_entry(entry: &str) -> Option<(u64, i64)> {
    let (count, at) = entry.split_once(':')?;
    Some((count.parse().ok()?, at.parse().ok()?))
}

/// Encodes a version vector as `"device=count,device=count"`.
fn encode_vector(vector: &VersionVector) -> String {
    vector
        .iter()
        .map(|(device, count)| format!("{device}={count}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn decode_vector(encoded: &str) -> VersionVector {
    encoded
        .split(',')
        .filter_map(|pair| {
            let (device, count) = pair.split_once('=')?;
            Some((device.parse().ok()?, count.parse().ok()?))
        })
        .collect()
}

fn out_as_str(value: &Out) -> Option<&str> {
    match value {
        Out::Any(Any::String(s)) => Some(s.as_ref()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry written by a later version, in a layout this one cannot
    /// read, is not shown as an attachment. The note names the file all
    /// the same: a device that left it out would in time tell the server
    /// the file was no longer wanted.
    #[test]
    fn a_file_whose_entry_cannot_be_read_is_still_named() {
        let doc = NoteDoc::new(7_100_000_000_000_001);
        doc.set_text("the whiteboard", 1_000);
        let later = uuid::Uuid::now_v7();
        {
            let mut txn = doc.doc.transact_mut();
            doc.files
                .insert(&mut txn, later.to_string(), "{\"layout\":2}".to_owned());
            // Not a file's ID at all: named by nothing.
            doc.files
                .insert(&mut txn, "not an id".to_owned(), "{}".to_owned());
        }

        assert!(doc.attachments().is_empty());
        assert_eq!(doc.named_files(), [later]);
        // And it survives being stored and loaded.
        let loaded = NoteDoc::from_bytes(7_100_000_000_000_002, &doc.encode()).unwrap();
        assert_eq!(loaded.named_files(), [later]);
    }
}
