//! Sync module.
//!
//! `merge` holds the pure conflict-matrix resolver used by every client
//! driver. `api_client` is the HTTP wrapper around the bearer-authed
//! `dirt-api` backend.

pub mod api_client;
pub mod merge;
