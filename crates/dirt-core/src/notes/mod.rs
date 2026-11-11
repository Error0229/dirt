//! Notes as CRDT documents.
//!
//! Each note is one document that can be merged with any other copy of
//! itself, in any order, and always ends up the same. The text, the tags, the
//! last-edited time and whether the note is deleted are all read from the
//! document. Design sections 6.1 and 6.2 are the specification; the mechanism
//! was proven in trial 1 (`docs/trials/2026-11-10-trial-1-yrs-document-size.md`).

mod diff;
mod doc;

pub use doc::{DocError, NoteDoc, VersionVector};
