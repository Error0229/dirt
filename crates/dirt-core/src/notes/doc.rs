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
        Self {
            doc,
            body,
            edits,
            deletions,
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

    /// Brings a deleted note back. This is an ordinary edit.
    pub fn restore(&self, now_ms: i64) {
        let mut txn = self.doc.transact_mut();
        self.record_edit(&mut txn, now_ms);
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
