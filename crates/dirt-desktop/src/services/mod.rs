//! Application services
//!
//! Services for database access and other shared functionality.

mod auth;
mod database;
mod transcription;

pub use auth::{AuthConfigStatus, AuthSession, SignUpOutcome, SupabaseAuthService};
pub use database::DatabaseService;
#[allow(unused_imports)] // Exported for follow-up voice memo wiring.
pub use transcription::{TranscriptionConfigStatus, TranscriptionError, TranscriptionService};
