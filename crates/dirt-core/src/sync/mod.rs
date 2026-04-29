//! Sync module.
//!
//! `merge` holds the pure conflict-matrix resolver used by every client
//! driver. `api_client` is the HTTP wrapper around the bearer-authed
//! `dirt-api` backend. `engine` ties them together with the local
//! `pending_sync` / `sync_state` tables to run a single push+pull cycle.

pub mod api_client;
pub mod engine;
pub mod merge;
