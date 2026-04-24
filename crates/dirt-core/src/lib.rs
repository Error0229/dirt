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
pub mod services;
pub mod state;
pub mod storage;
pub mod sync;
pub mod util;

pub use error::{Error, Result};
pub use export::ExportNote;
pub use models::{Note, NoteId};
pub use state::SyncState;

/// Solo-phase tenant sentinel.
///
/// Every note written on the local client and every row stored server-side
/// carries this user id until Phase 2 replaces it with real authenticated
/// identities. Referenced by migrations, repository inserts, and the
/// server-side auth middleware so the constant never gets copy-pasted and
/// drift across call sites.
pub const SOLO_USER_ID: &str = "01932a0c-3f8b-7e4c-8b1d-3a9c2f5e1234";
