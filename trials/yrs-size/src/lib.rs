//! Trial 1 of the Dirt redesign.
//!
//! A prototype of the note document described in section 6 of
//! `docs/plans/2026-11-10-system-redesign-design.md`: one yrs document per
//! note, edits applied as text diffs, and a deletion rule under which a delete
//! covers only what the deleting device had seen.
//!
//! This is trial code. It exists to measure document size and to prove the
//! merge and deletion behaviour before `dirt-core` is rebuilt on it.

pub mod diff;
pub mod doc;

pub use doc::{NoteDoc, VersionVector};
