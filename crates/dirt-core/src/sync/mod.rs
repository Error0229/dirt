//! Sync: the cycle that exchanges notes with the server, and the worker that
//! decides when to run it.
//!
//! - [`engine`]: one cycle of pull, decrypt and merge, then encrypt and push.
//! - [`worker`]: runs cycles in the background when something happens.

pub mod engine;
pub mod worker;
