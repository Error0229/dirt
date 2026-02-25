//! Application services
//!
//! Services for database access and other shared functionality.
//! Auth, sync, and media clients are shared from dirt-core;
//! only platform-specific wiring (session store, database wrapper) lives here.

mod database;
mod export;
mod session_store;
mod transcription;
mod voice_memo;

// Re-export shared types from dirt-core
pub use dirt_core::auth::{
    AuthConfigStatus, AuthError, AuthResult, AuthSession, SignUpOutcome,
    SupabaseAuthService,
};
pub use dirt_core::config::BootstrapConfig;
pub use dirt_core::media::MediaApiClient;
pub use dirt_core::sync::TursoSyncAuthClient;
pub use dirt_core::util::normalize_text_option;

/// Desktop auth service wired to the OS keyring for session persistence.
pub type DesktopAuthService = SupabaseAuthService<KeyringSessionStore>;

/// Create a desktop auth service from bootstrap config.
pub fn auth_service_from_bootstrap(
    config: &BootstrapConfig,
) -> AuthResult<Option<DesktopAuthService>> {
    let url = normalize_text_option(config.supabase_url.clone());
    let anon_key = normalize_text_option(config.supabase_anon_key.clone());

    match (url, anon_key) {
        (None, None) => Ok(None),
        (Some(url), Some(anon_key)) => {
            let service =
                SupabaseAuthService::with_session_store(url, anon_key, KeyringSessionStore::default())?;
            Ok(Some(service))
        }
        _ => Err(AuthError::NotConfigured),
    }
}

/// Create a sync auth client from bootstrap config.
pub fn sync_auth_from_bootstrap(
    config: &BootstrapConfig,
) -> Result<Option<TursoSyncAuthClient>, dirt_core::sync::SyncAuthError> {
    let Some(endpoint) = config.turso_sync_token_endpoint.clone() else {
        return Ok(None);
    };
    Ok(Some(TursoSyncAuthClient::new(endpoint)?))
}

/// Create a media API client from bootstrap config.
pub fn media_client_from_bootstrap(
    config: &BootstrapConfig,
) -> Result<Option<MediaApiClient>, String> {
    let Some(base_url) = config.managed_api_base_url() else {
        return Ok(None);
    };
    Ok(Some(MediaApiClient::new(base_url)?))
}

// Re-export desktop-specific services
pub use database::DatabaseService;
pub use export::{export_notes_to_path, suggested_export_file_name, NotesExportFormat};
pub use session_store::KeyringSessionStore;
pub use transcription::{TranscriptionConfigStatus, TranscriptionService};
pub use voice_memo::{
    cleanup_temp_voice_memo, discard_voice_memo_recording, start_voice_memo_recording,
    stop_voice_memo_recording, transition_voice_memo_state, VoiceMemoRecorderEvent,
    VoiceMemoRecorderState,
};
