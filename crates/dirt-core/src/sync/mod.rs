//! Sync: the cycle that exchanges notes with the server, and the worker that
//! decides when to run it.
//!
//! - [`engine`]: one cycle of pull, decrypt and merge, then encrypt and push.
//! - [`worker`]: runs cycles in the background when something happens.
//! - [`overview`]: what every interface shows about sync, in one set of
//!   words.

pub mod engine;
pub mod overview;
pub mod worker;
