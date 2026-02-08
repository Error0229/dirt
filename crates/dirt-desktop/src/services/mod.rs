//! Application services
//!
//! Services for database access and other shared functionality.

mod auth;
mod database;
mod export;
mod transcription;

pub use auth::{AuthConfigStatus, AuthSession, SignUpOutcome, SupabaseAuthService};
pub use database::DatabaseService;
pub use export::{export_notes_to_path, suggested_export_file_name, NotesExportFormat};
#[allow(unused_imports)] // Exported for follow-up voice memo wiring.
pub use transcription::{TranscriptionConfigStatus, TranscriptionError, TranscriptionService};
