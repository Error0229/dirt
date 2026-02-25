use std::sync::Arc;
use std::time::{Duration, Instant};

use dioxus::prelude::*;
use dioxus_primitives::label::Label;
use dioxus_primitives::scroll_area::{ScrollArea, ScrollDirection, ScrollType};
use dioxus_primitives::separator::Separator;
use dioxus_primitives::toast::{use_toast, ToastOptions};
use dirt_core::{Attachment, AttachmentId, Note, NoteId, SyncConflict, SyncState};

use crate::attachments::{
    attachment_kind_label, build_attachment_preview, infer_attachment_mime_type, AttachmentPreview,
};
use crate::auth::{AuthConfigStatus, AuthSession, SignUpOutcome, SupabaseAuthService};
use crate::bootstrap_config::{
    load_bootstrap_config, resolve_bootstrap_config, MobileBootstrapConfig,
};
use crate::config::{
    load_runtime_config, resolve_sync_config, runtime_turso_token_status, save_runtime_config,
    MobileRuntimeConfig, SecretStatus, SyncConfigSource,
};
use crate::data::MobileNoteStore;
use crate::export::{
    default_export_directory, export_notes_to_path, suggested_export_file_name, MobileExportFormat,
};
use crate::filters::{collect_note_tags, filter_notes};
use crate::launch::LaunchIntent;
use crate::media_api::MediaApiClient;
use crate::secret_store;
use crate::sync_auth::{SyncToken, TursoSyncAuthClient};
use crate::ui::{ButtonVariant, UiButton, UiInput, UiTextarea, MOBILE_UI_STYLES};
use crate::voice_memo::{
    cleanup_temp_voice_memo, discard_voice_memo_recording, start_voice_memo_recording,
    stop_voice_memo_recording, transition_voice_memo_state, VoiceMemoRecorderEvent,
    VoiceMemoRecorderState,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MobileView {
    List,
    Editor,
    Settings,
}

type MobileSyncState = SyncState;

struct MobileConfigDiagnostics {
    auth_bootstrap_configured: bool,
    runtime_sync_url_configured: bool,
    managed_sync_endpoint_configured: bool,
    managed_media_configured: bool,
    turso_active_source: String,
    turso_managed_auth_endpoint: String,
    turso_runtime_endpoint: String,
    turso_runtime_token_status: String,
    supabase_url: String,
    supabase_anon_key_status: String,
    supabase_auth_status: String,
    r2_bucket: String,
    r2_endpoint: String,
    r2_credentials_status: String,
}

struct MobileProvisioningStatus {
    auth_status: String,
    auth_action: Option<String>,
    sync_status: String,
    sync_action: Option<String>,
    media_status: String,
    media_action: Option<String>,
}

const KIB_BYTES: u64 = 1024;
const MIB_BYTES: u64 = KIB_BYTES * 1024;
const GIB_BYTES: u64 = MIB_BYTES * 1024;
const SYNC_INTERVAL_SECS: u64 = 30;
const SYNC_CONFLICT_LIMIT: usize = 10;
const TOAST_STYLES: &str = r#"
.toast-container {
    position: fixed;
    inset: auto 12px 12px 12px;
    z-index: 9999;
    pointer-events: none;
}
.toast-list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
}
.toast {
    pointer-events: auto;
    border-radius: 10px;
    border: 1px solid #d1d5db;
    background: #ffffff;
    box-shadow: 0 10px 30px rgba(17, 24, 39, 0.12);
    padding: 10px 12px;
    color: #111827;
    display: flex;
    gap: 10px;
    align-items: flex-start;
}
.toast[data-type='success'] { border-color: #10b981; }
.toast[data-type='error'] { border-color: #ef4444; }
.toast[data-type='warning'] { border-color: #f59e0b; }
.toast[data-type='info'] { border-color: #3b82f6; }
.toast-content { flex: 1; }
.toast-title { font-size: 13px; font-weight: 700; }
.toast-description { font-size: 12px; color: #4b5563; margin-top: 2px; }
.toast-close {
    border: 0;
    background: transparent;
    color: #6b7280;
    font-size: 16px;
    line-height: 1;
    padding: 0;
}
"#;

#[component]
pub(crate) fn AppShell() -> Element {
    let mut store = use_signal(|| None::<Arc<MobileNoteStore>>);
    let mut notes = use_signal(Vec::<Note>::new);
    let mut search_query = use_signal(String::new);
    let mut active_tag_filter = use_signal(|| None::<String>);
    let mut selected_note_id = use_signal(|| None::<NoteId>);
    let mut draft_content = use_signal(String::new);
    let mut draft_dirty = use_signal(|| false);
    let mut draft_edit_version = use_signal(|| 0u64);
    let mut view = use_signal(|| MobileView::List);
    let mut status_message = use_signal(|| None::<String>);
    let mut loading = use_signal(|| true);
    let mut saving = use_signal(|| false);
    let mut deleting = use_signal(|| false);
    let mut sync_state = use_signal(|| MobileSyncState::Offline);
    let mut last_sync_at = use_signal(|| None::<i64>);
    let mut sync_scheduler_active = use_signal(|| false);
    let mut last_sync_attempt_at = use_signal(|| None::<i64>);
    let mut consecutive_sync_failures = use_signal(|| 0u32);
    let mut pending_sync_count = use_signal(|| 0usize);
    let mut pending_sync_note_ids = use_signal(Vec::<NoteId>::new);
    let mut sync_conflicts = use_signal(Vec::<SyncConflict>::new);
    let mut sync_conflicts_loading = use_signal(|| false);
    let mut sync_conflicts_error = use_signal(|| None::<String>);
    let mut sync_conflicts_refresh_version = use_signal(|| 0u64);
    let mut note_attachments = use_signal(Vec::<Attachment>::new);
    let mut attachments_loading = use_signal(|| false);
    let mut attachments_error = use_signal(|| None::<String>);
    let mut attachment_uploading = use_signal(|| false);
    let mut attachment_upload_error = use_signal(|| None::<String>);
    let mut deleting_attachment_id = use_signal(|| None::<AttachmentId>);
    let mut attachment_preview_open = use_signal(|| false);
    let mut attachment_preview_loading = use_signal(|| false);
    let mut attachment_preview_title = use_signal(String::new);
    let mut attachment_preview_content = use_signal(AttachmentPreview::default);
    let mut attachment_preview_error = use_signal(|| None::<String>);
    let mut attachment_refresh_version = use_signal(|| 0u64);
    let mut voice_memo_state = use_signal(VoiceMemoRecorderState::default);
    let mut voice_memo_started_at = use_signal(|| None::<Instant>);
    let mut db_init_retry_version = use_signal(|| 0u64);
    let mut turso_database_url_input = use_signal(String::new);
    let mut openai_api_key_input = use_signal(String::new);
    let mut openai_api_key_configured = use_signal(|| false);
    let mut active_sync_source = use_signal(|| SyncConfigSource::None);
    let mut auth_service = use_signal(|| None::<Arc<SupabaseAuthService>>);
    let mut auth_session = use_signal(|| None::<AuthSession>);
    let mut media_api_client = use_signal(|| None::<Arc<MediaApiClient>>);
    let mut sync_auth_client = use_signal(|| None::<Arc<TursoSyncAuthClient>>);
    let mut sync_token_expires_at = use_signal(|| None::<i64>);
    let mut auth_email_input = use_signal(String::new);
    let mut auth_password_input = use_signal(String::new);
    let mut auth_config_status = use_signal(|| None::<AuthConfigStatus>);
    let mut auth_loading = use_signal(|| false);
    let mut export_busy = use_signal(|| false);
    let launch: Signal<LaunchIntent> = use_signal(crate::launch::detect_launch_intent_from_runtime);
    let mut launch_applied = use_signal(|| false);
    let bootstrap_config = load_bootstrap_config();
    let toasts = use_toast();

    let bootstrap_config_for_init = bootstrap_config.clone();
    use_future(move || {
        let bootstrap_config = bootstrap_config_for_init.clone();
        async move {
            let bootstrap_config = resolve_bootstrap_config(bootstrap_config).await;
            let _db_init_retry_version = db_init_retry_version();
            let runtime_config = load_runtime_config();
            let runtime_has_sync_url = runtime_config.has_sync_url();
            turso_database_url_input.set(
                runtime_config
                    .turso_database_url
                    .clone()
                    .unwrap_or_default(),
            );
            match secret_store::read_secret(secret_store::SECRET_OPENAI_API_KEY) {
                Ok(secret) => openai_api_key_configured.set(secret.is_some()),
                Err(error) => {
                    tracing::warn!(
                        "Failed to read OpenAI API key from secure storage: {}",
                        error
                    );
                    openai_api_key_configured.set(false);
                }
            }
            let mut resolved_sync_config = resolve_sync_config();
            active_sync_source.set(resolved_sync_config.source);
            auth_service.set(None);
            auth_session.set(None);
            auth_config_status.set(None);
            media_api_client.set(None);
            sync_auth_client.set(None);
            sync_token_expires_at.set(None);

            match MediaApiClient::new_from_bootstrap(&bootstrap_config) {
                Ok(Some(client)) => media_api_client.set(Some(Arc::new(client))),
                Ok(None) => {}
                Err(error) => {
                    status_message
                        .set(Some(format!("Managed media API is misconfigured: {error}")));
                }
            }

            match TursoSyncAuthClient::new_from_bootstrap(&bootstrap_config) {
                Ok(Some(client)) => {
                    sync_auth_client.set(Some(Arc::new(client)));
                }
                Ok(None) => {}
                Err(error) => {
                    status_message
                        .set(Some(format!("Managed sync auth is misconfigured: {error}")));
                }
            }

            match SupabaseAuthService::new_from_bootstrap(&bootstrap_config) {
                Ok(Some(service)) => {
                    let service = Arc::new(service);
                    match service.restore_session().await {
                        Ok(session) => auth_session.set(session.clone()),
                        Err(error) => {
                            tracing::warn!("Failed to restore mobile auth session: {}", error);
                            status_message
                                .set(Some(format!("Auth session restore failed: {error}")));
                        }
                    }
                    match service.verify_configuration().await {
                        Ok(config) => auth_config_status.set(Some(config)),
                        Err(error) => {
                            tracing::warn!("Mobile auth config verification failed: {}", error);
                            status_message
                                .set(Some(format!("Auth configuration check failed: {error}")));
                        }
                    }
                    auth_service.set(Some(service));
                }
                Ok(None) => {}
                Err(error) => {
                    status_message.set(Some(format!("Auth is not configured: {error}")));
                }
            }

            if runtime_has_sync_url {
                if sync_auth_client.read().is_some() && auth_session().is_none() {
                    if let Err(error) =
                        secret_store::delete_secret(secret_store::SECRET_TURSO_AUTH_TOKEN)
                    {
                        tracing::warn!(
                            "Failed to clear stale managed sync token without active session: {}",
                            error
                        );
                    }
                }
                match refresh_managed_sync_token(
                    sync_auth_client.read().clone(),
                    auth_session(),
                    &mut status_message,
                )
                .await
                {
                    Ok(Some(token)) => {
                        sync_token_expires_at.set(Some(token.expires_at));
                        resolved_sync_config = resolve_sync_config();
                        active_sync_source.set(resolved_sync_config.source);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        status_message
                            .set(Some(format!("Managed sync token exchange failed: {error}")));
                    }
                }
            }

            loading.set(true);
            store.set(None);
            notes.set(Vec::new());
            sync_state.set(MobileSyncState::Offline);
            last_sync_at.set(None);
            sync_scheduler_active.set(false);
            last_sync_attempt_at.set(None);
            consecutive_sync_failures.set(0);
            clear_pending_sync_queue(&mut pending_sync_note_ids, &mut pending_sync_count);
            let launch = launch();
            let mut initialized = false;

            match MobileNoteStore::open_default().await {
                Ok(note_store) => {
                    let note_store = Arc::new(note_store);
                    initialized = true;

                    store.set(Some(note_store.clone()));

                    if note_store.is_sync_enabled().await {
                        sync_scheduler_active.set(true);
                        sync_state.set(MobileSyncState::Syncing);
                        last_sync_attempt_at.set(Some(chrono::Utc::now().timestamp_millis()));
                        match note_store.sync().await {
                            Ok(()) => {
                                sync_state.set(MobileSyncState::Synced);
                                last_sync_at.set(Some(chrono::Utc::now().timestamp_millis()));
                                consecutive_sync_failures.set(0);
                                clear_pending_sync_queue(
                                    &mut pending_sync_note_ids,
                                    &mut pending_sync_count,
                                );
                                toasts.info(
                                    "Sync connected".to_string(),
                                    ToastOptions::new().description(
                                        "Remote sync is active for this mobile database",
                                    ),
                                );
                            }
                            Err(error) => {
                                tracing::error!("Initial mobile sync failed: {}", error);
                                sync_state.set(MobileSyncState::Error);
                                consecutive_sync_failures.set(1);
                                status_message.set(Some(format!(
                                    "Initial sync failed; retrying in background: {error}"
                                )));
                                toasts.error(
                                    "Initial sync failed".to_string(),
                                    ToastOptions::new().description(
                                        "Changes will keep retrying in the background",
                                    ),
                                );
                            }
                        }
                    } else {
                        sync_scheduler_active.set(false);
                        sync_state.set(MobileSyncState::Offline);
                        let offline_message = resolved_sync_config.warning.clone().unwrap_or_else(|| {
                        "Running in local-only mode (set Turso URL and sign in to enable auto-sync)"
                            .to_string()
                    });
                        status_message.set(Some(offline_message));
                    }

                    match note_store.list_notes().await {
                        Ok(loaded_notes) => {
                            notes.set(loaded_notes);
                        }
                        Err(error) => {
                            status_message.set(Some(format!("Failed to load notes: {error}")));
                        }
                    }
                }
                Err(error) => {
                    sync_scheduler_active.set(false);
                    status_message.set(Some(format!("Failed to open database: {error}")));
                }
            }

            if initialized && !launch_applied() {
                if let Some(shared_text) = launch.share_text {
                    apply_share_intent(
                        shared_text,
                        &mut selected_note_id,
                        &mut draft_content,
                        &mut draft_dirty,
                        &mut draft_edit_version,
                        &mut status_message,
                    );
                    view.set(MobileView::Editor);
                    launch_applied.set(true);
                } else if launch.quick_capture.enabled {
                    apply_quick_capture_launch(
                        launch.quick_capture.seed_text,
                        &mut draft_content,
                        &mut draft_dirty,
                        &mut draft_edit_version,
                    );
                    selected_note_id.set(None);
                    status_message.set(Some("Quick capture ready".to_string()));
                    view.set(MobileView::Editor);
                    launch_applied.set(true);
                }
            }

            loading.set(false);
        }
    });

    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(SYNC_INTERVAL_SECS)).await;

            let managed_sync_enabled =
                sync_auth_client.read().is_some() && load_runtime_config().has_sync_url();
            if managed_sync_enabled
                && sync_token_expires_at()
                    .map(|expires_at| expires_at <= chrono::Utc::now().timestamp() + 60)
                    .unwrap_or(true)
            {
                match refresh_managed_sync_token(
                    sync_auth_client.read().clone(),
                    auth_session(),
                    &mut status_message,
                )
                .await
                {
                    Ok(Some(token)) => {
                        sync_token_expires_at.set(Some(token.expires_at));
                        status_message.set(Some(
                            "Refreshed sync credentials. Reinitializing database connection..."
                                .to_string(),
                        ));
                        db_init_retry_version.set(db_init_retry_version() + 1);
                        continue;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        status_message
                            .set(Some(format!("Managed sync token refresh failed: {error}")));
                    }
                }
            }

            let Some(note_store) = store.read().clone() else {
                sync_scheduler_active.set(false);
                continue;
            };

            if !note_store.is_sync_enabled().await {
                sync_scheduler_active.set(false);
                sync_state.set(MobileSyncState::Offline);
                continue;
            }

            sync_scheduler_active.set(true);
            let sync_attempt_timestamp = chrono::Utc::now().timestamp_millis();
            last_sync_attempt_at.set(Some(sync_attempt_timestamp));
            tracing::debug!(
                "Mobile sync scheduler tick: interval={}s, attempt_at={sync_attempt_timestamp}",
                SYNC_INTERVAL_SECS
            );

            let previous_sync_state = sync_state();
            sync_state.set(MobileSyncState::Syncing);

            match note_store.sync().await {
                Ok(()) => {
                    sync_state.set(MobileSyncState::Synced);
                    last_sync_at.set(Some(chrono::Utc::now().timestamp_millis()));
                    consecutive_sync_failures.set(0);
                    clear_pending_sync_queue(&mut pending_sync_note_ids, &mut pending_sync_count);

                    if previous_sync_state == MobileSyncState::Error {
                        toasts.success(
                            "Sync restored".to_string(),
                            ToastOptions::new()
                                .description("Remote sync recovered after a failure"),
                        );
                    }

                    if let Ok(fresh_notes) = note_store.list_notes().await {
                        notes.set(fresh_notes);
                    }
                }
                Err(error) => {
                    tracing::error!("Periodic mobile sync failed: {}", error);
                    sync_state.set(MobileSyncState::Error);
                    consecutive_sync_failures.set(consecutive_sync_failures().saturating_add(1));

                    if managed_sync_enabled && should_refresh_managed_token_after_sync_error(&error)
                    {
                        match refresh_managed_sync_token(
                            sync_auth_client.read().clone(),
                            auth_session(),
                            &mut status_message,
                        )
                        .await
                        {
                            Ok(Some(token)) => {
                                sync_token_expires_at.set(Some(token.expires_at));
                                status_message.set(Some(
                                    "Recovered sync credentials after auth error. Reinitializing database connection..."
                                        .to_string(),
                                ));
                                db_init_retry_version.set(db_init_retry_version() + 1);
                                continue;
                            }
                            Ok(None) => {}
                            Err(refresh_error) => {
                                status_message.set(Some(format!(
                                    "Sync auth refresh failed after error: {refresh_error}"
                                )));
                            }
                        }
                    }

                    if previous_sync_state != MobileSyncState::Error {
                        toasts.error(
                            "Sync failed".to_string(),
                            ToastOptions::new().description("Will continue retrying automatically"),
                        );
                    }
                }
            }
        }
    });

    use_future(move || async move {
        let selected_note_id = selected_note_id();
        let _attachment_refresh_version = attachment_refresh_version();

        let Some(note_store) = store.read().clone() else {
            note_attachments.set(Vec::new());
            attachments_error.set(None);
            attachments_loading.set(false);
            return;
        };

        let Some(note_id) = selected_note_id else {
            note_attachments.set(Vec::new());
            attachments_error.set(None);
            attachments_loading.set(false);
            return;
        };

        attachments_loading.set(true);
        attachments_error.set(None);

        match note_store.list_attachments(&note_id).await {
            Ok(attachments) => {
                note_attachments.set(attachments);
            }
            Err(error) => {
                note_attachments.set(Vec::new());
                attachments_error.set(Some(format!("Failed to load attachments: {error}")));
            }
        }

        attachments_loading.set(false);
    });

    use_future(move || async move {
        let current_view = view();
        let current_note_id = selected_note_id();
        let current_voice_state = voice_memo_state();

        if current_voice_state == VoiceMemoRecorderState::Idle {
            return;
        }

        if current_view == MobileView::Editor && current_note_id.is_some() {
            return;
        }

        if current_voice_state == VoiceMemoRecorderState::Stopping {
            return;
        }

        voice_memo_state.set(VoiceMemoRecorderState::Idle);
        voice_memo_started_at.set(None);
        if let Err(error) = discard_voice_memo_recording().await {
            attachment_upload_error.set(Some(format!(
                "Failed to stop voice memo recorder while changing view: {error}"
            )));
        }
    });

    use_future(move || async move {
        let _sync_conflicts_refresh_version = sync_conflicts_refresh_version();
        if view() != MobileView::Settings {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            sync_conflicts.set(Vec::new());
            sync_conflicts_error.set(Some(
                "Sync details will appear once initialization finishes.".to_string(),
            ));
            sync_conflicts_loading.set(false);
            return;
        };

        sync_conflicts_loading.set(true);
        sync_conflicts_error.set(None);

        match note_store.list_conflicts(SYNC_CONFLICT_LIMIT).await {
            Ok(conflicts) => {
                sync_conflicts.set(conflicts);
            }
            Err(error) => {
                sync_conflicts.set(Vec::new());
                sync_conflicts_error.set(Some(format!("Failed to load conflicts: {error}")));
            }
        }

        sync_conflicts_loading.set(false);
    });

    let on_new_note = move |_| {
        if store.read().is_none() {
            status_message.set(Some(
                "Still initializing your notes. Please try again in a moment.".to_string(),
            ));
            return;
        }
        selected_note_id.set(None);
        draft_content.set(String::new());
        draft_dirty.set(false);
        status_message.set(None);
        attachment_upload_error.set(None);
        attachment_preview_open.set(false);
        view.set(MobileView::Editor);
    };

    let on_retry_db_init = move |_| {
        if loading() {
            return;
        }
        status_message.set(Some("Retrying database initialization...".to_string()));
        db_init_retry_version.set(db_init_retry_version() + 1);
    };

    let on_back_to_list = move |_| {
        view.set(MobileView::List);
        attachment_preview_open.set(false);
        attachment_preview_loading.set(false);
        attachment_preview_error.set(None);
    };

    let on_open_settings = move |_| {
        view.set(MobileView::Settings);
        sync_conflicts_refresh_version.set(sync_conflicts_refresh_version().saturating_add(1));
    };

    let on_refresh_sync_conflicts = move |_| {
        if sync_conflicts_loading() {
            return;
        }
        sync_conflicts_refresh_version.set(sync_conflicts_refresh_version().saturating_add(1));
    };

    let on_save_sync_settings = move |_| {
        let runtime_config = MobileRuntimeConfig::from_raw(Some(turso_database_url_input()));
        if !runtime_config.has_sync_url() {
            status_message.set(Some(
                "Turso URL is required to enable remote sync".to_string(),
            ));
            return;
        }

        match save_runtime_config(&runtime_config) {
            Ok(()) => {
                active_sync_source.set(SyncConfigSource::RuntimeSettings);
                status_message.set(Some(
                    "Sync settings saved. Reconnecting in background.".to_string(),
                ));
                db_init_retry_version.set(db_init_retry_version() + 1);
            }
            Err(error) => {
                status_message.set(Some(format!("Failed to save sync settings: {error}")));
            }
        }
    };

    let on_clear_sync_settings = move |_| {
        let empty_config = MobileRuntimeConfig::default();
        if let Err(error) = save_runtime_config(&empty_config) {
            status_message.set(Some(format!("Failed to clear sync settings: {error}")));
            return;
        }
        if let Err(error) = secret_store::delete_secret(secret_store::SECRET_TURSO_AUTH_TOKEN) {
            status_message.set(Some(format!(
                "Failed to clear secure Turso token; URL was cleared: {error}"
            )));
            return;
        }

        turso_database_url_input.set(String::new());
        active_sync_source.set(SyncConfigSource::None);
        sync_token_expires_at.set(None);
        status_message.set(Some(
            "Cleared runtime sync settings. Reinitializing database connection...".to_string(),
        ));
        db_init_retry_version.set(db_init_retry_version() + 1);
    };

    let on_save_openai_api_key = move |_| {
        let api_key = openai_api_key_input().trim().to_string();
        if api_key.is_empty() {
            status_message.set(Some("OpenAI API key is required.".to_string()));
            return;
        }

        match secret_store::write_secret(secret_store::SECRET_OPENAI_API_KEY, &api_key) {
            Ok(()) => {
                openai_api_key_input.set(String::new());
                openai_api_key_configured.set(true);
                status_message.set(Some(
                    "OpenAI API key saved to secure device storage.".to_string(),
                ));
            }
            Err(error) => {
                status_message.set(Some(format!("Failed to save OpenAI API key: {error}")));
            }
        }
    };

    let on_clear_openai_api_key =
        move |_| match secret_store::delete_secret(secret_store::SECRET_OPENAI_API_KEY) {
            Ok(()) => {
                openai_api_key_input.set(String::new());
                openai_api_key_configured.set(false);
                status_message.set(Some("OpenAI API key cleared.".to_string()));
            }
            Err(error) => {
                status_message.set(Some(format!("Failed to clear OpenAI API key: {error}")));
            }
        };

    let on_auth_sign_in = move |_| {
        if auth_loading() {
            return;
        }
        let Some(service) = auth_service.read().clone() else {
            status_message.set(Some(
                "Supabase auth is not configured in mobile bootstrap.".to_string(),
            ));
            return;
        };

        let email = auth_email_input().trim().to_string();
        let password = auth_password_input().trim().to_string();
        if email.is_empty() || password.is_empty() {
            status_message.set(Some("Email and password are required".to_string()));
            return;
        }

        auth_loading.set(true);
        status_message.set(Some("Signing in...".to_string()));

        spawn(async move {
            match service.sign_in(&email, &password).await {
                Ok(session) => {
                    let session_email = session
                        .user
                        .email
                        .clone()
                        .unwrap_or_else(|| "unknown user".to_string());
                    auth_session.set(Some(session.clone()));
                    auth_password_input.set(String::new());
                    status_message.set(Some(format!("Signed in as {session_email}")));

                    match refresh_managed_sync_token(
                        sync_auth_client.read().clone(),
                        Some(session),
                        &mut status_message,
                    )
                    .await
                    {
                        Ok(Some(token)) => {
                            sync_token_expires_at.set(Some(token.expires_at));
                            status_message.set(Some(
                                "Signed in and refreshed sync credentials.".to_string(),
                            ));
                            db_init_retry_version.set(db_init_retry_version() + 1);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            status_message.set(Some(format!(
                                "Signed in, but sync token refresh failed: {error}"
                            )));
                        }
                    }

                    if let Ok(config) = service.verify_configuration().await {
                        auth_config_status.set(Some(config));
                    }
                }
                Err(error) => {
                    status_message.set(Some(format!("Sign-in failed: {error}")));
                }
            }
            auth_loading.set(false);
        });
    };

    let on_auth_sign_up = move |_| {
        if auth_loading() {
            return;
        }
        let Some(service) = auth_service.read().clone() else {
            status_message.set(Some(
                "Supabase auth is not configured in mobile bootstrap.".to_string(),
            ));
            return;
        };

        let email = auth_email_input().trim().to_string();
        let password = auth_password_input().trim().to_string();
        if email.is_empty() || password.is_empty() {
            status_message.set(Some("Email and password are required".to_string()));
            return;
        }

        auth_loading.set(true);
        status_message.set(Some("Signing up...".to_string()));

        spawn(async move {
            match service.sign_up(&email, &password).await {
                Ok(SignUpOutcome::SignedIn(session)) => {
                    let session_email = session
                        .user
                        .email
                        .clone()
                        .unwrap_or_else(|| "unknown user".to_string());
                    auth_session.set(Some(session.clone()));
                    auth_password_input.set(String::new());
                    status_message.set(Some(format!("Signed up and signed in as {session_email}")));

                    match refresh_managed_sync_token(
                        sync_auth_client.read().clone(),
                        Some(session),
                        &mut status_message,
                    )
                    .await
                    {
                        Ok(Some(token)) => {
                            sync_token_expires_at.set(Some(token.expires_at));
                            status_message.set(Some(
                                "Signed up and refreshed sync credentials.".to_string(),
                            ));
                            db_init_retry_version.set(db_init_retry_version() + 1);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            status_message.set(Some(format!(
                                "Signed up, but sync token refresh failed: {error}"
                            )));
                        }
                    }
                }
                Ok(SignUpOutcome::ConfirmationRequired) => {
                    status_message.set(Some(
                        "Sign-up succeeded. Check your email to confirm the account.".to_string(),
                    ));
                }
                Err(error) => {
                    status_message.set(Some(format!("Sign-up failed: {error}")));
                }
            }
            auth_loading.set(false);
        });
    };

    let on_auth_sign_out = move |_| {
        if auth_loading() {
            return;
        }
        let Some(service) = auth_service.read().clone() else {
            status_message.set(Some("Auth service is unavailable".to_string()));
            return;
        };
        let Some(session) = auth_session() else {
            status_message.set(Some("No active session to sign out".to_string()));
            return;
        };

        auth_loading.set(true);
        status_message.set(Some("Signing out...".to_string()));

        spawn(async move {
            match service.sign_out(&session.access_token).await {
                Ok(()) => {
                    auth_session.set(None);
                    auth_password_input.set(String::new());
                    sync_token_expires_at.set(None);
                    if let Err(error) =
                        secret_store::delete_secret(secret_store::SECRET_TURSO_AUTH_TOKEN)
                    {
                        status_message.set(Some(format!(
                            "Signed out, but failed to clear cached sync token: {error}"
                        )));
                    } else {
                        status_message.set(Some("Signed out".to_string()));
                    }
                    db_init_retry_version.set(db_init_retry_version() + 1);
                }
                Err(error) => {
                    status_message.set(Some(format!("Sign-out failed: {error}")));
                }
            }
            auth_loading.set(false);
        });
    };

    let on_export_json = move |_| {
        if export_busy() {
            return;
        }
        let Some(note_store) = store.read().clone() else {
            status_message.set(Some(
                "Still initializing your notes. Please try again in a moment.".to_string(),
            ));
            return;
        };

        export_busy.set(true);
        status_message.set(Some("Exporting notes as JSON...".to_string()));

        let output_path = default_export_directory().join(suggested_export_file_name(
            MobileExportFormat::Json,
            chrono::Utc::now().timestamp_millis(),
        ));

        spawn(async move {
            match export_notes_to_path(note_store, MobileExportFormat::Json, &output_path).await {
                Ok(note_count) => {
                    status_message.set(Some(format!(
                        "Exported {note_count} notes to {}",
                        output_path.display()
                    )));
                }
                Err(error) => {
                    status_message.set(Some(format!("JSON export failed: {error}")));
                }
            }
            export_busy.set(false);
        });
    };

    let on_export_markdown = move |_| {
        if export_busy() {
            return;
        }
        let Some(note_store) = store.read().clone() else {
            status_message.set(Some(
                "Still initializing your notes. Please try again in a moment.".to_string(),
            ));
            return;
        };

        export_busy.set(true);
        status_message.set(Some("Exporting notes as Markdown...".to_string()));

        let output_path = default_export_directory().join(suggested_export_file_name(
            MobileExportFormat::Markdown,
            chrono::Utc::now().timestamp_millis(),
        ));

        spawn(async move {
            match export_notes_to_path(note_store, MobileExportFormat::Markdown, &output_path).await
            {
                Ok(note_count) => {
                    status_message.set(Some(format!(
                        "Exported {note_count} notes to {}",
                        output_path.display()
                    )));
                }
                Err(error) => {
                    status_message.set(Some(format!("Markdown export failed: {error}")));
                }
            }
            export_busy.set(false);
        });
    };

    use_future(move || async move {
        let current_revision = draft_edit_version();
        if !draft_dirty() {
            return;
        }

        tokio::time::sleep(Duration::from_millis(650)).await;
        if current_revision != draft_edit_version() || !draft_dirty() {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            return;
        };
        if saving() {
            return;
        }

        let content = draft_content().trim().to_string();
        if content.is_empty() {
            return;
        }

        let current_note_id = selected_note_id();
        saving.set(true);

        let save_result = if let Some(note_id) = current_note_id {
            note_store.update_note(&note_id, &content).await
        } else {
            note_store.create_note(&content).await
        };

        match save_result {
            Ok(saved_note) => {
                selected_note_id.set(Some(saved_note.id));
                draft_content.set(saved_note.content);
                if current_revision == draft_edit_version() {
                    draft_dirty.set(false);
                }
                enqueue_pending_sync_change(
                    saved_note.id,
                    &mut pending_sync_note_ids,
                    &mut pending_sync_count,
                );

                match note_store.list_notes().await {
                    Ok(fresh_notes) => notes.set(fresh_notes),
                    Err(error) => {
                        status_message.set(Some(format!(
                            "Saved, but failed to refresh note list: {error}"
                        )));
                    }
                }
            }
            Err(error) => {
                status_message.set(Some(format!("Failed to save note: {error}")));
            }
        }

        saving.set(false);
    });

    let on_delete_note = move |_| {
        if deleting() {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            status_message.set(Some(
                "Still initializing your notes. Please try again in a moment.".to_string(),
            ));
            return;
        };
        let Some(note_id) = selected_note_id() else {
            status_message.set(Some("Select a note to delete".to_string()));
            return;
        };

        deleting.set(true);
        status_message.set(Some("Deleting note...".to_string()));

        spawn(async move {
            match note_store.delete_note(&note_id).await {
                Ok(()) => {
                    enqueue_pending_sync_change(
                        note_id,
                        &mut pending_sync_note_ids,
                        &mut pending_sync_count,
                    );
                    selected_note_id.set(None);
                    draft_content.set(String::new());
                    view.set(MobileView::List);

                    match note_store.list_notes().await {
                        Ok(fresh_notes) => {
                            notes.set(fresh_notes);
                            status_message.set(Some("Note deleted".to_string()));
                        }
                        Err(error) => {
                            status_message.set(Some(format!(
                                "Deleted, but failed to refresh list: {error}"
                            )));
                        }
                    }
                }
                Err(error) => {
                    status_message.set(Some(format!("Failed to delete note: {error}")));
                }
            }

            deleting.set(false);
        });
    };

    let on_pick_attachment = move |event: Event<FormData>| {
        if attachment_uploading() {
            return;
        }

        let Some(note_id) = selected_note_id() else {
            attachment_upload_error.set(Some(
                "Save this note before uploading attachments.".to_string(),
            ));
            return;
        };
        let Some(note_store) = store.read().clone() else {
            attachment_upload_error.set(Some(
                "Still initializing your notes. Please try again in a moment.".to_string(),
            ));
            return;
        };

        let mut files = event.files();
        let Some(file) = files.pop() else {
            return;
        };

        let file_name = file.name();
        if file_name.trim().is_empty() {
            attachment_upload_error.set(Some("Selected file has no name.".to_string()));
            return;
        }
        let file_content_type = file.content_type();

        attachment_upload_error.set(None);
        attachment_uploading.set(true);
        status_message.set(Some(format!("Uploading {file_name}...")));

        let media_api = media_api_client.read().clone();
        let auth_session_value = auth_session();
        spawn(async move {
            let file_bytes = match file.read_bytes().await {
                Ok(bytes) => bytes.to_vec(),
                Err(error) => {
                    attachment_upload_error
                        .set(Some(format!("Failed to read selected file: {error}")));
                    attachment_uploading.set(false);
                    return;
                }
            };

            match upload_attachment_to_r2(
                note_store,
                note_id,
                file_name,
                file_content_type,
                file_bytes,
                media_api,
                auth_session_value,
            )
            .await
            {
                Ok(()) => {
                    enqueue_pending_sync_change(
                        note_id,
                        &mut pending_sync_note_ids,
                        &mut pending_sync_count,
                    );
                    attachment_refresh_version.set(attachment_refresh_version() + 1);
                    status_message.set(Some("Attachment uploaded.".to_string()));
                }
                Err(error) => {
                    attachment_upload_error.set(Some(error.clone()));
                    status_message.set(Some(error));
                }
            }

            attachment_uploading.set(false);
        });
    };

    let on_start_voice_memo = move |_| {
        attachment_upload_error.set(None);

        if attachment_uploading() || voice_memo_state() != VoiceMemoRecorderState::Idle {
            return;
        }

        let Some(_note_id) = selected_note_id() else {
            attachment_upload_error.set(Some(
                "Save this note before recording a voice memo.".to_string(),
            ));
            return;
        };

        voice_memo_state.set(transition_voice_memo_state(
            voice_memo_state(),
            VoiceMemoRecorderEvent::StartRequested,
        ));

        spawn(async move {
            match start_voice_memo_recording().await {
                Ok(()) => {
                    voice_memo_state.set(transition_voice_memo_state(
                        voice_memo_state(),
                        VoiceMemoRecorderEvent::StartSucceeded,
                    ));
                    voice_memo_started_at.set(Some(Instant::now()));
                }
                Err(error) => {
                    voice_memo_state.set(transition_voice_memo_state(
                        voice_memo_state(),
                        VoiceMemoRecorderEvent::StartFailed,
                    ));
                    voice_memo_started_at.set(None);
                    attachment_upload_error.set(Some(format!(
                        "Voice memo recording failed to start: {error}"
                    )));
                }
            }
        });
    };

    let on_stop_voice_memo = move |_| {
        attachment_upload_error.set(None);

        if attachment_uploading() || voice_memo_state() != VoiceMemoRecorderState::Recording {
            return;
        }

        let Some(note_id) = selected_note_id() else {
            attachment_upload_error.set(Some(
                "Save this note before attaching a voice memo.".to_string(),
            ));
            return;
        };

        let Some(note_store) = store.read().clone() else {
            attachment_upload_error.set(Some(
                "Still initializing your notes. Please try again in a moment.".to_string(),
            ));
            return;
        };

        voice_memo_state.set(transition_voice_memo_state(
            voice_memo_state(),
            VoiceMemoRecorderEvent::StopRequested,
        ));

        let media_api = media_api_client.read().clone();
        let auth_session_value = auth_session();
        spawn(async move {
            match stop_voice_memo_recording().await {
                Ok(recorded) => {
                    let upload_result = upload_attachment_to_r2(
                        note_store,
                        note_id,
                        recorded.file_name.clone(),
                        Some(recorded.mime_type.clone()),
                        recorded.bytes,
                        media_api,
                        auth_session_value,
                    )
                    .await;

                    cleanup_temp_voice_memo(recorded.temp_path.as_path());
                    match upload_result {
                        Ok(()) => {
                            enqueue_pending_sync_change(
                                note_id,
                                &mut pending_sync_note_ids,
                                &mut pending_sync_count,
                            );
                            attachment_refresh_version
                                .set(attachment_refresh_version().saturating_add(1));
                            status_message.set(Some("Voice memo attached.".to_string()));
                            voice_memo_state.set(transition_voice_memo_state(
                                voice_memo_state(),
                                VoiceMemoRecorderEvent::StopSucceeded,
                            ));
                            voice_memo_started_at.set(None);
                        }
                        Err(error) => {
                            attachment_upload_error.set(Some(error.clone()));
                            status_message.set(Some(error));
                            voice_memo_state.set(transition_voice_memo_state(
                                voice_memo_state(),
                                VoiceMemoRecorderEvent::StopFailed,
                            ));
                            voice_memo_started_at.set(None);
                        }
                    }
                }
                Err(error) => {
                    voice_memo_state.set(transition_voice_memo_state(
                        voice_memo_state(),
                        VoiceMemoRecorderEvent::StopFailed,
                    ));
                    voice_memo_started_at.set(None);
                    attachment_upload_error
                        .set(Some(format!("Failed to finalize voice memo: {error}")));
                }
            }
        });
    };

    let on_discard_voice_memo = move |_| {
        attachment_upload_error.set(None);

        if voice_memo_state() == VoiceMemoRecorderState::Idle {
            return;
        }

        voice_memo_state.set(transition_voice_memo_state(
            voice_memo_state(),
            VoiceMemoRecorderEvent::DiscardRequested,
        ));
        voice_memo_started_at.set(None);

        spawn(async move {
            if let Err(error) = discard_voice_memo_recording().await {
                attachment_upload_error.set(Some(format!(
                    "Failed to discard voice memo recording: {error}"
                )));
            } else {
                status_message.set(Some("Voice memo recording discarded.".to_string()));
            }
        });
    };

    let on_close_attachment_preview = move |_| {
        attachment_preview_open.set(false);
        attachment_preview_loading.set(false);
        attachment_preview_error.set(None);
        attachment_preview_content.set(AttachmentPreview::None);
    };

    let diagnostics = mobile_config_diagnostics(
        active_sync_source(),
        auth_config_status(),
        &bootstrap_config,
    );
    let current_auth_session = auth_session();
    let provisioning = mobile_provisioning_status(
        &diagnostics,
        current_auth_session.as_ref(),
        sync_state(),
        sync_scheduler_active(),
    );
    let heading = if view() == MobileView::Settings {
        "Settings"
    } else {
        "Dirt"
    };
    let sync_state_text = sync_state_label(sync_state(), last_sync_at());
    let last_sync_text = last_sync_at()
        .map(relative_time)
        .unwrap_or_else(|| "never".to_string());
    let last_sync_attempt_text = last_sync_attempt_at()
        .map(relative_time)
        .unwrap_or_else(|| "never".to_string());
    let sync_scheduler_text = if sync_scheduler_active() {
        format!("active (every {SYNC_INTERVAL_SECS}s)")
    } else {
        "inactive".to_string()
    };
    let pending_sync_count_value = pending_sync_count();
    let pending_sync_preview = format_pending_title(&pending_sync_note_ids());
    let auth_session_summary = current_auth_session
        .as_ref()
        .map(|session| {
            session
                .user
                .email
                .clone()
                .unwrap_or_else(|| format!("user {}", session.user.id))
        })
        .unwrap_or_else(|| "Not signed in".to_string());
    let auth_config_summary_text = auth_config_status()
        .map(auth_config_summary)
        .unwrap_or_else(|| "unknown".to_string());
    let all_notes = notes();
    let search_query_value = search_query();
    let active_tag_filter_value = active_tag_filter();
    let available_tags = collect_note_tags(&all_notes);
    let filtered_notes = filter_notes(
        &all_notes,
        &search_query_value,
        active_tag_filter_value.as_deref(),
    );
    let total_note_count = all_notes.len();
    let filtered_note_count = filtered_notes.len();
    let has_active_note_filters =
        !search_query_value.trim().is_empty() || active_tag_filter_value.is_some();
    let export_directory = default_export_directory();
    let export_directory_text = export_directory.display().to_string();
    let app_version = env!("CARGO_PKG_VERSION");
    let package_name = env!("CARGO_PKG_NAME");
    let voice_memo_state_value = voice_memo_state();
    let voice_memo_status = match voice_memo_state_value {
        VoiceMemoRecorderState::Idle => None,
        VoiceMemoRecorderState::Starting => Some("Requesting microphone access...".to_string()),
        VoiceMemoRecorderState::Recording => {
            let elapsed = voice_memo_started_at().map_or(0_u64, elapsed_millis_u64);
            Some(format!(
                "Recording voice memo... {}",
                format_recording_duration(elapsed)
            ))
        }
        VoiceMemoRecorderState::Stopping => Some("Finalizing voice memo...".to_string()),
    };

    rsx! {
        style {
            "{TOAST_STYLES}{MOBILE_UI_STYLES}"
        }

        div {
            style: "
                height: 100vh;
                display: flex;
                flex-direction: column;
                background: #f6f8fb;
                color: #111827;
                font-family: system-ui, sans-serif;
            ",

            div {
                style: "
                    padding: 14px 16px;
                    display: flex;
                    justify-content: space-between;
                    align-items: center;
                    background: #ffffff;
                ",
                h1 {
                    style: "margin: 0; font-size: 22px;",
                    "{heading}"
                }
                div {
                    style: "display: flex; flex-direction: column; align-items: flex-end; gap: 6px;",
                    if let Some(sync_label) = sync_state_banner_label(sync_state(), last_sync_at()) {
                        p {
                            style: "margin: 0; color: #4b5563; font-size: 11px;",
                            "{sync_label}"
                        }
                    }
                    if view() == MobileView::Settings {
                        UiButton {
                            type: "button",
                            variant: ButtonVariant::Outline,
                            style: "padding: 6px 10px; font-size: 12px;",
                            onclick: on_back_to_list,
                            "Notes"
                        }
                    } else {
                        UiButton {
                            type: "button",
                            variant: ButtonVariant::Outline,
                            style: "padding: 6px 10px; font-size: 12px;",
                            onclick: on_open_settings,
                            "Settings"
                        }
                    }
                }
            }

            Separator {
                decorative: true,
                style: "height: 1px; background: #e5e7eb;",
            }

            if let Some(message) = status_message() {
                p {
                    style: "margin: 0; padding: 10px 16px; font-size: 13px; color: #374151;",
                    "{message}"
                }
                Separator {
                    decorative: true,
                    style: "height: 1px; background: #e5e7eb;",
                }
            }

            if loading() {
                div {
                    style: "
                        flex: 1;
                        display: flex;
                        align-items: center;
                        justify-content: center;
                        color: #6b7280;
                    ",
                    "Loading notes..."
                }
            } else if view() == MobileView::List {
                {include!("views/list.rs")}
            } else if view() == MobileView::Settings {
                {include!("views/settings.rs")}
            } else {
                {include!("views/editor.rs")}
            }
        }
    }
}

fn apply_quick_capture_launch(
    seed_text: Option<String>,
    draft_content: &mut Signal<String>,
    draft_dirty: &mut Signal<bool>,
    draft_edit_version: &mut Signal<u64>,
) {
    draft_content.set(seed_text.unwrap_or_default());
    draft_dirty.set(true);
    draft_edit_version.set(draft_edit_version().saturating_add(1));
}

fn apply_share_intent(
    shared_text: String,
    selected_note_id: &mut Signal<Option<NoteId>>,
    draft_content: &mut Signal<String>,
    draft_dirty: &mut Signal<bool>,
    draft_edit_version: &mut Signal<u64>,
    status_message: &mut Signal<Option<String>>,
) {
    selected_note_id.set(None);
    draft_content.set(shared_text);
    draft_dirty.set(true);
    draft_edit_version.set(draft_edit_version().saturating_add(1));
    status_message.set(Some("Shared text ready".to_string()));
}

async fn upload_attachment_to_r2(
    note_store: Arc<MobileNoteStore>,
    note_id: NoteId,
    file_name: String,
    content_type: Option<String>,
    file_bytes: Vec<u8>,
    media_api: Option<Arc<MediaApiClient>>,
    auth_session: Option<AuthSession>,
) -> Result<(), String> {
    let media_api = media_api.ok_or_else(|| {
        "Managed media API is not configured in bootstrap. Set DIRT_API_BASE_URL.".to_string()
    })?;
    let access_token = require_media_access_token(auth_session)?;
    let object_key = build_media_object_key(&note_id, &file_name);
    let mime_type = infer_attachment_mime_type(content_type.as_deref(), &file_name);

    media_api
        .upload(&access_token, &object_key, &mime_type, file_bytes.as_ref())
        .await
        .map_err(|error| format!("Failed to upload attachment via media API: {error}"))?;

    note_store
        .create_attachment(
            &note_id,
            &file_name,
            &mime_type,
            file_size_i64(file_bytes.len()),
            &object_key,
        )
        .await
        .map(|_| ())
        .map_err(|error| format!("Failed to save attachment metadata: {error}"))
}

async fn load_attachment_preview_from_r2(
    attachment: &Attachment,
    media_api: Option<Arc<MediaApiClient>>,
    auth_session: Option<AuthSession>,
) -> Result<AttachmentPreview, String> {
    let media_api = media_api.ok_or_else(|| {
        "Managed media API is not configured in bootstrap. Set DIRT_API_BASE_URL.".to_string()
    })?;
    let access_token = require_media_access_token(auth_session)?;
    let (bytes, downloaded_content_type) = media_api
        .download(&access_token, &attachment.r2_key)
        .await
        .map_err(|error| format!("Failed to download attachment via media API: {error}"))?;

    let content_type_hint = downloaded_content_type
        .as_deref()
        .or(Some(attachment.mime_type.as_str()));
    let mime_type = infer_attachment_mime_type(content_type_hint, &attachment.filename);

    Ok(build_attachment_preview(
        &attachment.filename,
        &mime_type,
        &bytes,
    ))
}

async fn delete_attachment_object_from_r2(
    object_key: &str,
    media_api: Option<Arc<MediaApiClient>>,
    auth_session: Option<AuthSession>,
) -> Result<(), String> {
    let media_api = media_api.ok_or_else(|| {
        "Managed media API is not configured in bootstrap. Set DIRT_API_BASE_URL.".to_string()
    })?;
    let access_token = require_media_access_token(auth_session)?;
    media_api
        .delete(&access_token, object_key)
        .await
        .map_err(|error| format!("{error}"))
}

fn require_media_access_token(auth_session: Option<AuthSession>) -> Result<String, String> {
    auth_session
        .map(|session| session.access_token)
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| "Sign in is required for managed attachment operations.".to_string())
}

fn build_media_object_key(note_id: &NoteId, file_name: &str) -> String {
    let stem = file_name
        .trim()
        .rsplit_once('.')
        .map_or(file_name.trim(), |(left, _)| left);
    let ext = file_name
        .trim()
        .rsplit_once('.')
        .map_or("", |(_, right)| right);

    let safe_stem = sanitize_media_token(stem);
    let safe_stem = if safe_stem.is_empty() {
        "file".to_string()
    } else {
        safe_stem
    };
    let safe_ext = sanitize_media_token(ext);
    let safe_name = if safe_ext.is_empty() {
        safe_stem
    } else {
        format!("{safe_stem}.{safe_ext}")
    };
    let now = chrono::Utc::now().timestamp_millis();
    format!("notes/{note_id}/{now}-{safe_name}")
}

fn sanitize_media_token(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;

    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }

    out.trim_matches('-').to_string()
}

fn render_attachment_preview(preview: AttachmentPreview, preview_title: &str) -> Element {
    match preview {
        AttachmentPreview::None => rsx! {
            p {
                style: "margin: 0; font-size: 12px; color: #6b7280;",
                "No preview available."
            }
        },
        AttachmentPreview::Text { content, truncated } => rsx! {
            div {
                if truncated {
                    p {
                        style: "margin: 0 0 8px 0; font-size: 12px; color: #6b7280;",
                        "Preview truncated to 256 KB."
                    }
                }
                pre {
                    style: "margin: 0; white-space: pre-wrap; word-break: break-word; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; line-height: 1.5;",
                    "{content}"
                }
            }
        },
        AttachmentPreview::MediaDataUri {
            mime_type,
            data_uri,
        } => rsx! {
            if mime_type.starts_with("image/") {
                img {
                    src: "{data_uri}",
                    alt: "{preview_title}",
                    style: "display: block; max-width: 100%; max-height: 60vh; margin: 0 auto; border-radius: 8px;",
                }
            } else if mime_type.starts_with("video/") {
                video {
                    src: "{data_uri}",
                    controls: true,
                    style: "display: block; width: 100%; max-height: 60vh; border-radius: 8px;",
                }
            } else if mime_type.starts_with("audio/") {
                audio {
                    src: "{data_uri}",
                    controls: true,
                    style: "width: 100%;",
                }
            }
        },
        AttachmentPreview::Unsupported { mime_type, reason } => rsx! {
            div {
                style: "display: flex; flex-direction: column; gap: 6px;",
                p {
                    style: "margin: 0; font-size: 12px; color: #4b5563;",
                    "MIME type: {mime_type}"
                }
                p {
                    style: "margin: 0; font-size: 12px; color: #6b7280;",
                    "{reason}"
                }
            }
        },
    }
}

fn enqueue_pending_sync_change(
    note_id: NoteId,
    pending_sync_note_ids: &mut Signal<Vec<NoteId>>,
    pending_sync_count: &mut Signal<usize>,
) {
    let mut pending_notes = pending_sync_note_ids.write();
    if !pending_notes.contains(&note_id) {
        pending_notes.push(note_id);
        pending_sync_count.set(pending_notes.len());
    }
}

fn clear_pending_sync_queue(
    pending_sync_note_ids: &mut Signal<Vec<NoteId>>,
    pending_sync_count: &mut Signal<usize>,
) {
    pending_sync_note_ids.write().clear();
    pending_sync_count.set(0);
}

fn format_pending_title(note_ids: &[NoteId]) -> String {
    if note_ids.is_empty() {
        return "none".to_string();
    }

    let preview = note_ids
        .iter()
        .take(5)
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    if note_ids.len() > 5 {
        format!("{preview}, +{}", note_ids.len() - 5)
    } else {
        preview
    }
}

fn format_sync_conflict_time(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms).map_or_else(
        || timestamp_ms.to_string(),
        |date_time| date_time.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
}

fn sync_state_label(state: MobileSyncState, last_sync_at: Option<i64>) -> String {
    match state {
        MobileSyncState::Offline => "Sync: local-only mode".to_string(),
        MobileSyncState::Syncing => "Sync: syncing...".to_string(),
        MobileSyncState::Synced => last_sync_at
            .map(|timestamp| format!("Sync: updated {}", relative_time(timestamp)))
            .unwrap_or_else(|| "Sync: connected".to_string()),
        MobileSyncState::Error => format!("Sync: retrying every {SYNC_INTERVAL_SECS}s after error"),
    }
}

fn sync_state_banner_label(state: MobileSyncState, last_sync_at: Option<i64>) -> Option<String> {
    match state {
        MobileSyncState::Offline => None,
        _ => Some(sync_state_label(state, last_sync_at)),
    }
}

fn mobile_config_diagnostics(
    active_sync_source: SyncConfigSource,
    auth_config: Option<AuthConfigStatus>,
    bootstrap_config: &MobileBootstrapConfig,
) -> MobileConfigDiagnostics {
    let runtime_config = load_runtime_config();
    let runtime_sync_url_configured = runtime_config.has_sync_url();
    let turso_runtime_url = runtime_config.turso_database_url;
    let turso_runtime_token_status = runtime_turso_token_status();
    let managed_sync_endpoint = bootstrap_config.turso_sync_token_endpoint.clone();
    let supabase_url = bootstrap_config.supabase_url.clone();
    let supabase_anon_key_set = bootstrap_config.supabase_anon_key.is_some();
    let auth_bootstrap_configured = supabase_url.is_some() && supabase_anon_key_set;

    let managed_api_base = bootstrap_config.managed_api_base_url();
    let managed_sync_endpoint_configured = managed_sync_endpoint.is_some();
    let managed_media_configured = managed_api_base.is_some();

    MobileConfigDiagnostics {
        auth_bootstrap_configured,
        runtime_sync_url_configured,
        managed_sync_endpoint_configured,
        managed_media_configured,
        turso_active_source: sync_config_source_label(active_sync_source).to_string(),
        turso_managed_auth_endpoint: managed_sync_endpoint
            .as_deref()
            .map(mask_endpoint_value)
            .unwrap_or_else(|| "not set".to_string()),
        turso_runtime_endpoint: turso_runtime_url
            .as_deref()
            .map(mask_endpoint_value)
            .unwrap_or_else(|| "not set".to_string()),
        turso_runtime_token_status: secure_status_label(&turso_runtime_token_status),
        supabase_url: supabase_url
            .as_deref()
            .map(mask_endpoint_value)
            .unwrap_or_else(|| "not set".to_string()),
        supabase_anon_key_status: configured_status_label(supabase_anon_key_set).to_string(),
        supabase_auth_status: auth_config
            .map(auth_config_summary)
            .unwrap_or_else(|| "unknown".to_string()),
        r2_bucket: if managed_media_configured {
            "managed (backend)".to_string()
        } else {
            "not set".to_string()
        },
        r2_endpoint: managed_api_base
            .as_deref()
            .map(mask_endpoint_value)
            .unwrap_or_else(|| "not set".to_string()),
        r2_credentials_status: if managed_media_configured {
            "managed (backend)".to_string()
        } else {
            "not set".to_string()
        },
    }
}

fn mobile_provisioning_status(
    diagnostics: &MobileConfigDiagnostics,
    auth_session: Option<&AuthSession>,
    sync_state: MobileSyncState,
    sync_scheduler_active: bool,
) -> MobileProvisioningStatus {
    let signed_in = auth_session.is_some();

    let (auth_status, auth_action) = if !diagnostics.auth_bootstrap_configured {
        (
            "Unavailable".to_string(),
            Some("This build is missing auth provisioning.".to_string()),
        )
    } else if signed_in {
        ("Configured and signed in".to_string(), None)
    } else {
        (
            "Configured (sign in required)".to_string(),
            Some("Sign in to enable cloud sync and attachment access.".to_string()),
        )
    };

    let (sync_status, sync_action) = if !diagnostics.runtime_sync_url_configured {
        (
            "Local-only".to_string(),
            Some("Add your Turso URL in Sync settings to enable cloud sync.".to_string()),
        )
    } else if !diagnostics.managed_sync_endpoint_configured {
        (
            "Unavailable".to_string(),
            Some("This build is missing managed sync provisioning.".to_string()),
        )
    } else if !signed_in {
        (
            "Waiting for sign-in".to_string(),
            Some("Sign in to fetch short-lived sync credentials.".to_string()),
        )
    } else if sync_scheduler_active {
        match sync_state {
            MobileSyncState::Synced => ("Active".to_string(), None),
            MobileSyncState::Syncing => ("Connecting".to_string(), None),
            MobileSyncState::Error => (
                "Retrying automatically".to_string(),
                Some("Sync failures are retried in the background.".to_string()),
            ),
            MobileSyncState::Offline => ("Connecting".to_string(), None),
        }
    } else if sync_state == MobileSyncState::Error {
        (
            "Retrying automatically".to_string(),
            Some("Sync failures are retried in the background.".to_string()),
        )
    } else {
        ("Paused".to_string(), None)
    };

    let (media_status, media_action) = if !diagnostics.managed_media_configured {
        (
            "Unavailable".to_string(),
            Some("Attachment cloud operations are not provisioned for this build.".to_string()),
        )
    } else if !signed_in {
        (
            "Configured (sign in required)".to_string(),
            Some("Sign in to upload, open, or delete cloud attachments.".to_string()),
        )
    } else {
        ("Available".to_string(), None)
    };

    MobileProvisioningStatus {
        auth_status,
        auth_action,
        sync_status,
        sync_action,
        media_status,
        media_action,
    }
}

fn sync_config_source_label(source: SyncConfigSource) -> &'static str {
    match source {
        SyncConfigSource::RuntimeSettings => "runtime settings",
        SyncConfigSource::None => "none",
    }
}

fn auth_config_summary(status: AuthConfigStatus) -> String {
    let email = if status.email_enabled {
        "email:on"
    } else {
        "email:off"
    };
    let signup = if status.signup_enabled {
        "signup:on"
    } else {
        "signup:off"
    };
    let confirm = if status.mailer_autoconfirm {
        "autoconfirm:on"
    } else {
        "autoconfirm:off"
    };
    format!("{email}, {signup}, {confirm}")
}

async fn refresh_managed_sync_token(
    sync_auth_client: Option<Arc<TursoSyncAuthClient>>,
    auth_session: Option<AuthSession>,
    _status_message: &mut Signal<Option<String>>,
) -> Result<Option<SyncToken>, String> {
    let Some(client) = sync_auth_client else {
        return Ok(None);
    };
    if !load_runtime_config().has_sync_url() {
        return Ok(None);
    }
    let Some(session) = auth_session else {
        return Ok(None);
    };

    let token = client
        .exchange_token(&session.access_token)
        .await
        .map_err(|error| error.to_string())?;
    secret_store::write_secret(secret_store::SECRET_TURSO_AUTH_TOKEN, &token.token)
        .map_err(|error| format!("Failed to persist managed sync token: {error}"))?;
    Ok(Some(token))
}

fn should_refresh_managed_token_after_sync_error(error: &dirt_core::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("auth")
        || message.contains("token")
        || message.contains("unauthorized")
        || message.contains("forbidden")
        || message.contains("permission denied")
}

fn configured_status_label(is_configured: bool) -> &'static str {
    if is_configured {
        "configured"
    } else {
        "not set"
    }
}

fn secure_status_label(status: &SecretStatus) -> String {
    match status {
        SecretStatus::Present => "configured".to_string(),
        SecretStatus::Missing => "not set".to_string(),
        SecretStatus::Error(error) => format!("error ({error})"),
    }
}

fn mask_endpoint_value(raw: &str) -> String {
    if let Some((scheme, rest)) = raw.split_once("://") {
        let host = rest.split('/').next().unwrap_or(rest);
        if host.is_empty() {
            raw.to_string()
        } else {
            format!("{scheme}://{host}")
        }
    } else {
        raw.split('/').next().unwrap_or(raw).to_string()
    }
}

fn note_title(note: &Note) -> String {
    let title = note.title_preview(48);
    if title.trim().is_empty() {
        "Untitled note".to_string()
    } else {
        title
    }
}

fn note_preview(note: &Note) -> String {
    let preview = note
        .content
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .chars()
        .take(80)
        .collect::<String>();

    if preview.is_empty() {
        "Tap to open".to_string()
    } else {
        preview
    }
}

fn format_attachment_size(size_bytes: i64) -> String {
    let bytes = u64::try_from(size_bytes).unwrap_or(0);

    if bytes < KIB_BYTES {
        format!("{bytes} B")
    } else if bytes < MIB_BYTES {
        format_scaled_one_decimal(bytes, KIB_BYTES, "KB")
    } else if bytes < GIB_BYTES {
        format_scaled_one_decimal(bytes, MIB_BYTES, "MB")
    } else {
        format_scaled_one_decimal(bytes, GIB_BYTES, "GB")
    }
}

fn format_scaled_one_decimal(bytes: u64, unit: u64, suffix: &str) -> String {
    let mut whole = bytes / unit;
    let mut tenth = ((bytes % unit) * 10 + (unit / 2)) / unit;

    if tenth == 10 {
        whole += 1;
        tenth = 0;
    }

    format!("{whole}.{tenth} {suffix}")
}

fn format_recording_duration(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1_000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn elapsed_millis_u64(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn file_size_i64(len: usize) -> i64 {
    i64::try_from(len).map_or(i64::MAX, |size| size)
}

fn relative_time(updated_at_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let delta = (now - updated_at_ms).max(0);

    if delta < 60_000 {
        "just now".to_string()
    } else if delta < 3_600_000 {
        format!("{}m ago", delta / 60_000)
    } else if delta < 86_400_000 {
        format!("{}h ago", delta / 3_600_000)
    } else {
        format!("{}d ago", delta / 86_400_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostics_fixture() -> MobileConfigDiagnostics {
        MobileConfigDiagnostics {
            auth_bootstrap_configured: true,
            runtime_sync_url_configured: true,
            managed_sync_endpoint_configured: true,
            managed_media_configured: true,
            turso_active_source: "runtime settings".to_string(),
            turso_managed_auth_endpoint: "https://api.example.com".to_string(),
            turso_runtime_endpoint: "libsql://example.turso.io".to_string(),
            turso_runtime_token_status: "configured".to_string(),
            supabase_url: "https://project.supabase.co".to_string(),
            supabase_anon_key_status: "configured".to_string(),
            supabase_auth_status: "email:on, signup:on, autoconfirm:off".to_string(),
            r2_bucket: "managed (backend)".to_string(),
            r2_endpoint: "https://api.example.com".to_string(),
            r2_credentials_status: "managed (backend)".to_string(),
        }
    }

    fn auth_session_fixture() -> AuthSession {
        AuthSession {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            expires_at: i64::MAX,
            user: crate::auth::AuthUser {
                id: "user-id".to_string(),
                email: Some("test@example.com".to_string()),
            },
        }
    }

    #[test]
    fn formats_attachment_sizes_for_mobile_ui() {
        assert_eq!(format_attachment_size(800), "800 B");
        assert_eq!(format_attachment_size(1_536), "1.5 KB");
        assert_eq!(format_attachment_size(3_145_728), "3.0 MB");
        assert_eq!(format_attachment_size(-1), "0 B");
    }

    #[test]
    fn formats_recording_duration_for_mobile_ui() {
        assert_eq!(format_recording_duration(0), "00:00");
        assert_eq!(format_recording_duration(59_999), "00:59");
        assert_eq!(format_recording_duration(125_001), "02:05");
    }

    #[test]
    fn maps_attachment_kind_labels() {
        assert_eq!(attachment_kind_label("photo.png", "image/png"), "image");
        assert_eq!(attachment_kind_label("voice.wav", "audio/wav"), "audio");
        assert_eq!(attachment_kind_label("clip.mp4", "video/mp4"), "video");
        assert_eq!(attachment_kind_label("notes.txt", "text/plain"), "text");
        assert_eq!(
            attachment_kind_label("report.pdf", "application/pdf"),
            "file"
        );
    }

    #[test]
    fn masks_configured_endpoints() {
        assert_eq!(
            mask_endpoint_value("libsql://dirt-main.aws-ap-northeast-1.turso.io?authToken=secret"),
            "libsql://dirt-main.aws-ap-northeast-1.turso.io"
        );
        assert_eq!(
            mask_endpoint_value("https://project.supabase.co/rest/v1"),
            "https://project.supabase.co"
        );
        assert_eq!(
            mask_endpoint_value("project.supabase.co/path"),
            "project.supabase.co"
        );
    }

    #[test]
    fn detects_sync_errors_that_should_trigger_managed_token_refresh() {
        assert!(should_refresh_managed_token_after_sync_error(
            &dirt_core::Error::Database("auth token expired".to_string())
        ));
        assert!(should_refresh_managed_token_after_sync_error(
            &dirt_core::Error::Database("unauthorized".to_string())
        ));
        assert!(!should_refresh_managed_token_after_sync_error(
            &dirt_core::Error::Database("disk I/O error".to_string())
        ));
    }

    #[test]
    fn formats_sync_conflict_timestamps_for_display() {
        assert_eq!(
            format_sync_conflict_time(0),
            "1970-01-01 00:00:00 UTC".to_string()
        );
    }

    #[test]
    fn formats_pending_note_title_preview() {
        let ids = vec![
            NoteId::new(),
            NoteId::new(),
            NoteId::new(),
            NoteId::new(),
            NoteId::new(),
            NoteId::new(),
        ];
        let title = format_pending_title(&ids);
        assert!(title.contains("+1"));
    }

    #[test]
    fn builds_managed_media_object_key() {
        let note_id = NoteId::new();
        let key = build_media_object_key(&note_id, "My File (Final).PNG");
        assert!(key.starts_with(&format!("notes/{note_id}/")));
        assert!(key.ends_with("-my-file-final.png"));
    }

    #[test]
    fn sanitizes_media_token() {
        assert_eq!(sanitize_media_token(" My  File__Name "), "my-file-name");
        assert_eq!(sanitize_media_token("..."), "");
    }

    #[test]
    fn provisioning_status_prompts_for_sign_in() {
        let diagnostics = diagnostics_fixture();
        let status =
            mobile_provisioning_status(&diagnostics, None, MobileSyncState::Offline, false);

        assert_eq!(status.auth_status, "Configured (sign in required)");
        assert_eq!(status.sync_status, "Waiting for sign-in");
        assert_eq!(status.media_status, "Configured (sign in required)");
        assert!(status.auth_action.is_some());
        assert!(status.sync_action.is_some());
        assert!(status.media_action.is_some());
    }

    #[test]
    fn provisioning_status_reports_active_when_ready() {
        let diagnostics = diagnostics_fixture();
        let session = auth_session_fixture();
        let status =
            mobile_provisioning_status(&diagnostics, Some(&session), MobileSyncState::Synced, true);

        assert_eq!(status.auth_status, "Configured and signed in");
        assert_eq!(status.sync_status, "Active");
        assert_eq!(status.media_status, "Available");
        assert!(status.auth_action.is_none());
        assert!(status.sync_action.is_none());
        assert!(status.media_action.is_none());
    }
}
