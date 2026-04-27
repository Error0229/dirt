//! Application services
//!
//! Shared functionality for database access and platform-specific wiring.
//! Auth and the Turso embedded-replica sync layer are gone — the new sync
//! path goes through `dirt_core::sync::api_client::ApiClient` and is wired
//! up by the per-client sync worker (not landed in this commit).

mod database;
mod export;
mod sync_worker;
mod transcription;

pub use sync_worker::{spawn_sync_worker, SyncEvent, SyncWorkerHandle};

// Re-export desktop-specific services
pub use database::DatabaseService;
pub use export::{export_notes_to_path, suggested_export_file_name, NotesExportFormat};
pub use transcription::{TranscriptionConfigStatus, TranscriptionService};
