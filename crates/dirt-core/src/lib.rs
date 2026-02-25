//! dirt-core - Core library for Dirt
//!
//! This crate contains the shared models, database layer, business logic,
//! and platform-agnostic service clients used by all Dirt interfaces
//! (desktop, mobile, CLI, TUI).

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod export;
pub mod media;
pub mod models;
pub mod search;
pub mod state;
pub mod storage;
pub mod sync;
pub mod util;

pub use error::{Error, Result};
pub use export::ExportNote;
pub use models::{Attachment, AttachmentId, Note, NoteId, SyncConflict};
pub use state::SyncState;
