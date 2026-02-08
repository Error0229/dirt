//! dirt-core - Core library for Dirt
//!
//! This crate contains the shared models, database layer, and business logic
//! used by all Dirt interfaces (desktop, mobile, CLI, TUI).

pub mod db;
pub mod error;
pub mod export;
pub mod models;
pub mod search;
pub mod storage;

pub use error::{Error, Result};
pub use export::ExportNote;
pub use models::{Attachment, AttachmentId, Note, NoteId, SyncConflict};
