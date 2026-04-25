//! `dirt sync` placeholder.
//!
//! The Turso embedded-replica sync path was removed in the Supabase
//! teardown. The follow-up commit wires this command to the
//! `dirt_core::sync::api_client::ApiClient` HTTP backend; until then
//! `dirt sync` returns `SyncNotConfigured` rather than silently no-oping.

use std::path::Path;

use crate::error::CliError;

// Kept `async` so the next commit (ApiClient-driven sync worker) can drop
// in real awaits without rewriting every call-site.
#[allow(clippy::unused_async)]
pub async fn run_sync(_db_path: &Path) -> Result<(), CliError> {
    Err(CliError::SyncNotConfigured)
}
