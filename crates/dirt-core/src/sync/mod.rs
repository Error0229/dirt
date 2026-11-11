//! HTTP clients for the server that the redesign replaces.
//!
//! Only the transport is left here: `api_client` talks to the old `dirt-api`
//! endpoints and `session_client` adds token refresh. The engine that applied
//! pulled notes to the local database was removed with that database in step
//! 3 of the redesign, so nothing syncs notes until step 5 brings the new
//! protocol. The sign-in flows still use these clients.

pub mod api_client;
pub mod engine;
pub mod session_client;
