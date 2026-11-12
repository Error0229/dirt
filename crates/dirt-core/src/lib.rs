//! dirt-core: everything the Dirt clients share.
//!
//! - [`notes`]: a note as a CRDT document, and what is derived from it.
//! - [`store`]: the local SQLite database of notes, tags, search and settings.
//! - [`export`]: notes as JSON or Markdown.
//! - [`auth`] and [`sync`]: the sign-in and HTTP clients for the server that
//!   the redesign replaces in steps 4 and 5. Nothing here syncs notes until
//!   then.
//!
//! The desktop app, the Android app and the CLI hold interface and platform
//! code only. No SQL lives outside [`store`].

pub mod api;
pub mod auth;
pub mod config;
pub mod crypto;
pub mod error;
pub mod export;
pub mod models;
pub mod notes;
pub mod store;
pub mod sync;
pub mod util;

pub use error::{Error, Result};
pub use export::ExportNote;
pub use models::{Note, NoteId};
pub use store::Store;
