//! dirt-core - Core library for Dirt
//!
//! This crate contains the shared models, database layer, and business logic
//! used by all Dirt interfaces (desktop, mobile, CLI, TUI).

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod export;
pub mod media;
pub mod models;
pub mod search;
pub mod services;
pub mod storage;
pub mod sync;

pub use error::{Error, Result};
pub use export::ExportNote;
pub use models::{Attachment, AttachmentId, Note, NoteId, SyncConflict};
