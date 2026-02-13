//! Application services
//!
//! Services for database access and other shared functionality.

mod auth;
mod database;
mod export;
mod media_api;
mod sync_auth;
mod transcription;
mod voice_memo;

pub use auth::{AuthConfigStatus, AuthSession, SignUpOutcome, SupabaseAuthService};
pub use database::DatabaseService;
pub use export::{export_notes_to_path, suggested_export_file_name, NotesExportFormat};
pub use media_api::MediaApiClient;
pub use sync_auth::TursoSyncAuthClient;
#[allow(unused_imports)] // Exported for follow-up voice memo wiring.
pub use transcription::{TranscriptionConfigStatus, TranscriptionError, TranscriptionService};
pub use voice_memo::{
    cleanup_temp_voice_memo, discard_voice_memo_recording, start_voice_memo_recording,
    stop_voice_memo_recording, transition_voice_memo_state, VoiceMemoRecorderEvent,
    VoiceMemoRecorderState,
};
