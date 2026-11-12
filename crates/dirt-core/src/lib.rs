//! dirt-core: everything the Dirt clients share.
//!
//! - [`notes`]: a note as a CRDT document, and what is derived from it.
//! - [`store`]: the local SQLite database of notes, tags, search, settings
//!   and sync state.
//! - [`crypto`]: the account key, note encryption and the device key pair.
//! - [`enrolment`]: the exchange that hands the account key to a new device.
//! - [`recovery`]: the recovery phrase.
//! - [`identity`]: signing in through the browser.
//! - [`secrets`]: the device's secure storage.
//! - [`api`]: the client for the Dirt server.
//! - [`sync`]: the sync cycle and the worker that schedules it.
//! - [`session`]: the one object a client holds: who is signed in, which
//!   store is in use, and sync.
//! - [`export`]: notes as JSON or Markdown.
//!
//! The desktop app, the Android app and the CLI hold interface and platform
//! code only. No SQL, HTTP or cryptography lives outside this crate.

pub mod api;
pub mod config;
pub mod crypto;
pub mod enrolment;
pub mod error;
pub mod export;
pub mod files;
pub mod identity;
pub mod key_change;
pub mod models;
pub mod notes;
pub mod recovery;
pub mod secrets;
pub mod session;
pub mod store;
pub mod sync;
pub mod util;

pub use error::{Error, Result};
pub use export::ExportNote;
pub use models::{Note, NoteId};
pub use store::Store;
