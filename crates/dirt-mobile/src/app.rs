use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use dioxus_primitives::label::Label;
use dioxus_primitives::scroll_area::{ScrollArea, ScrollDirection, ScrollType};
use dioxus_primitives::separator::Separator;
use dioxus_primitives::toast::{use_toast, ToastOptions, ToastProvider};
use dirt_core::storage::{MediaStorage, R2Config, R2Storage};
use dirt_core::{Attachment, AttachmentId, Note, NoteId, SyncConflict};

use crate::attachments::{
    attachment_kind_label, build_attachment_preview, infer_attachment_mime_type, AttachmentPreview,
};
use crate::auth::{AuthConfigStatus, AuthSession, SignUpOutcome, SupabaseAuthService};
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
use crate::secret_store;
use crate::sync_auth::{SyncToken, TursoSyncAuthClient};
use crate::ui::{ButtonVariant, UiButton, UiInput, UiTextarea, MOBILE_UI_STYLES};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MobileView {
    List,
    Editor,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MobileSyncState {
    Offline,
    Syncing,
    Synced,
    Error,
}

struct MobileConfigDiagnostics {
    turso_sync_configured: bool,
    turso_active_source: String,
    turso_managed_auth_endpoint: String,
    turso_runtime_endpoint: String,
    turso_runtime_token_status: String,
    turso_env_endpoint: String,
    turso_env_token_status: String,
    supabase_url: String,
    supabase_anon_key_status: String,
    supabase_auth_status: String,
    r2_bucket: String,
    r2_endpoint: String,
    r2_credentials_status: String,
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
pub fn App() -> Element {
    rsx! {
        ToastProvider {
            AppShell {}
        }
    }
}

#[component]
fn AppShell() -> Element {
    let mut store = use_signal(|| None::<Arc<MobileNoteStore>>);
    let mut notes = use_signal(Vec::<Note>::new);
    let mut search_query = use_signal(String::new);
    let mut active_tag_filter = use_signal(|| None::<String>);
    let mut selected_note_id = use_signal(|| None::<NoteId>);
    let mut draft_content = use_signal(String::new);
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
    let mut db_init_retry_version = use_signal(|| 0u64);
    let mut turso_database_url_input = use_signal(String::new);
    let mut active_sync_source = use_signal(|| SyncConfigSource::None);
    let mut auth_service = use_signal(|| None::<Arc<SupabaseAuthService>>);
    let mut auth_session = use_signal(|| None::<AuthSession>);
    let mut sync_auth_client = use_signal(|| None::<Arc<TursoSyncAuthClient>>);
    let mut sync_token_expires_at = use_signal(|| None::<i64>);
    let mut auth_email_input = use_signal(String::new);
    let mut auth_password_input = use_signal(String::new);
    let mut auth_config_status = use_signal(|| None::<AuthConfigStatus>);
    let mut auth_loading = use_signal(|| false);
    let mut export_busy = use_signal(|| false);
    let launch: Signal<LaunchIntent> = use_signal(crate::launch::detect_launch_intent_from_runtime);
    let mut launch_applied = use_signal(|| false);
    let toasts = use_toast();

    use_future(move || async move {
        let _db_init_retry_version = db_init_retry_version();
        let runtime_config = load_runtime_config();
        let runtime_has_sync_url = runtime_config.has_sync_url();
        turso_database_url_input.set(
            runtime_config
                .turso_database_url
                .clone()
                .unwrap_or_default(),
        );
        let mut resolved_sync_config = resolve_sync_config();
        active_sync_source.set(resolved_sync_config.source);
        auth_service.set(None);
        auth_session.set(None);
        auth_config_status.set(None);
        sync_auth_client.set(None);
        sync_token_expires_at.set(None);

        match TursoSyncAuthClient::new_from_env() {
            Ok(Some(client)) => {
                sync_auth_client.set(Some(Arc::new(client)));
            }
            Ok(None) => {}
            Err(error) => {
                status_message.set(Some(format!("Managed sync auth is misconfigured: {error}")));
            }
        }

        match SupabaseAuthService::new_from_env() {
            Ok(Some(service)) => {
                let service = Arc::new(service);
                match service.restore_session().await {
                    Ok(session) => auth_session.set(session.clone()),
                    Err(error) => {
                        tracing::warn!("Failed to restore mobile auth session: {}", error);
                        status_message.set(Some(format!("Auth session restore failed: {error}")));
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
                                ToastOptions::new()
                                    .description("Remote sync is active for this mobile database"),
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
                                ToastOptions::new()
                                    .description("Changes will keep retrying in the background"),
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
                    &mut status_message,
                );
                view.set(MobileView::Editor);
                launch_applied.set(true);
            } else if launch.quick_capture.enabled {
                apply_quick_capture_launch(launch.quick_capture.seed_text, &mut draft_content);
                selected_note_id.set(None);
                status_message.set(Some("Quick capture ready to save".to_string()));
                view.set(MobileView::Editor);
                launch_applied.set(true);
            }
        }

        loading.set(false);
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
        let _sync_conflicts_refresh_version = sync_conflicts_refresh_version();
        if view() != MobileView::Settings {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            sync_conflicts.set(Vec::new());
            sync_conflicts_error.set(Some("Database is not ready yet.".to_string()));
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
                "Database is not ready yet. Retry initialization first.".to_string(),
            ));
            return;
        }
        selected_note_id.set(None);
        draft_content.set(String::new());
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
                    "Saved sync settings. Sign in to fetch managed sync credentials, then retry initialization."
                        .to_string(),
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

    let on_auth_sign_in = move |_| {
        if auth_loading() {
            return;
        }
        let Some(service) = auth_service.read().clone() else {
            status_message.set(Some(
                "Supabase auth is not configured. Set SUPABASE_URL and SUPABASE_ANON_KEY."
                    .to_string(),
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
                "Supabase auth is not configured. Set SUPABASE_URL and SUPABASE_ANON_KEY."
                    .to_string(),
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
            status_message.set(Some("Database is not ready yet.".to_string()));
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
            status_message.set(Some("Database is not ready yet.".to_string()));
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

    let on_save_note = move |_| {
        if saving() {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            status_message.set(Some("Database is not ready yet".to_string()));
            return;
        };

        let content = draft_content().trim().to_string();
        if content.is_empty() {
            status_message.set(Some("Note content cannot be empty".to_string()));
            return;
        }

        let current_note_id = selected_note_id();
        saving.set(true);
        status_message.set(Some("Saving note...".to_string()));

        spawn(async move {
            let save_result = if let Some(note_id) = current_note_id {
                note_store.update_note(&note_id, &content).await
            } else {
                note_store.create_note(&content).await
            };

            match save_result {
                Ok(saved_note) => {
                    selected_note_id.set(Some(saved_note.id));
                    draft_content.set(saved_note.content);
                    enqueue_pending_sync_change(
                        saved_note.id,
                        &mut pending_sync_note_ids,
                        &mut pending_sync_count,
                    );

                    match note_store.list_notes().await {
                        Ok(fresh_notes) => {
                            notes.set(fresh_notes);
                            status_message.set(Some("Note saved".to_string()));
                        }
                        Err(error) => {
                            status_message
                                .set(Some(format!("Saved, but failed to refresh list: {error}")));
                        }
                    }
                }
                Err(error) => {
                    status_message.set(Some(format!("Failed to save note: {error}")));
                }
            }

            saving.set(false);
        });
    };

    let on_delete_note = move |_| {
        if deleting() {
            return;
        }

        let Some(note_store) = store.read().clone() else {
            status_message.set(Some("Database is not ready yet".to_string()));
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
            attachment_upload_error.set(Some("Database is not ready yet.".to_string()));
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

    let on_close_attachment_preview = move |_| {
        attachment_preview_open.set(false);
        attachment_preview_loading.set(false);
        attachment_preview_error.set(None);
        attachment_preview_content.set(AttachmentPreview::None);
    };

    let diagnostics = mobile_config_diagnostics(active_sync_source(), auth_config_status());
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
    let auth_session_summary = auth_session()
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
                if store.read().is_none() {
                    div {
                        style: "
                            flex: 1;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            padding: 20px;
                        ",
                        div {
                            style: "
                                width: 100%;
                                max-width: 360px;
                                background: #ffffff;
                                border: 1px solid #e5e7eb;
                                border-radius: 12px;
                                padding: 16px;
                                display: flex;
                                flex-direction: column;
                                gap: 10px;
                                color: #374151;
                            ",
                            p {
                                style: "margin: 0; font-size: 14px; font-weight: 600; color: #111827;",
                                "Database initialization failed"
                            }
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "Retry initialization to continue."
                            }
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Primary,
                                onclick: on_retry_db_init,
                                disabled: loading(),
                                "Retry"
                            }
                        }
                    }
                } else {
                    div {
                        style: "padding: 12px 16px; display: flex; gap: 8px;",
                        UiButton {
                            type: "button",
                            block: true,
                            variant: ButtonVariant::Secondary,
                            style: "font-size: 14px; padding: 12px;",
                            onclick: on_new_note,
                            "New note"
                        }
                    }

                    div {
                        style: "padding: 0 16px 12px 16px; display: flex; flex-direction: column; gap: 8px;",
                        Label {
                            html_for: "note-search",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Search"
                        }
                        UiInput {
                            id: "note-search",
                            r#type: "search",
                            placeholder: "Search notes...",
                            value: "{search_query_value}",
                            oninput: move |event: Event<FormData>| {
                                search_query.set(event.value());
                            },
                        }

                        if !available_tags.is_empty() {
                            div {
                                style: "display: flex; gap: 6px; flex-wrap: wrap;",
                                UiButton {
                                    type: "button",
                                    variant: if active_tag_filter_value.is_none() {
                                        ButtonVariant::Secondary
                                    } else {
                                        ButtonVariant::Outline
                                    },
                                    style: "padding: 6px 10px; font-size: 12px;",
                                    onclick: move |_| active_tag_filter.set(None),
                                    "All tags"
                                }
                                for tag in available_tags {
                                    {
                                        let tag_label = format!("#{tag}");
                                        let tag_value = tag.clone();
                                        let is_active =
                                            active_tag_filter_value.as_deref() == Some(tag.as_str());

                                        rsx! {
                                            UiButton {
                                                key: "{tag}",
                                                type: "button",
                                                variant: if is_active {
                                                    ButtonVariant::Secondary
                                                } else {
                                                    ButtonVariant::Outline
                                                },
                                                style: "padding: 6px 10px; font-size: 12px;",
                                                onclick: move |_| active_tag_filter.set(Some(tag_value.clone())),
                                                "{tag_label}"
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if has_active_note_filters {
                            div {
                                style: "display: flex; align-items: center; justify-content: space-between; gap: 8px;",
                                p {
                                    style: "margin: 0; font-size: 12px; color: #6b7280;",
                                    "Showing {filtered_note_count} of {total_note_count} notes"
                                }
                                UiButton {
                                    type: "button",
                                    variant: ButtonVariant::Outline,
                                    style: "padding: 6px 10px; font-size: 12px;",
                                    onclick: move |_| {
                                        search_query.set(String::new());
                                        active_tag_filter.set(None);
                                    },
                                    "Clear filters"
                                }
                            }
                        }
                    }

                    ScrollArea {
                        direction: ScrollDirection::Vertical,
                        scroll_type: ScrollType::Auto,
                        tabindex: "0",
                        style: "flex: 1; padding: 0 12px 16px 12px;",

                        if all_notes.is_empty() {
                            div {
                                style: "
                                    margin-top: 24px;
                                    padding: 20px;
                                    background: #ffffff;
                                    border: 1px solid #e5e7eb;
                                    border-radius: 12px;
                                    text-align: center;
                                    color: #6b7280;
                                ",
                                "No notes yet. Create your first note."
                            }
                        } else if filtered_notes.is_empty() {
                            div {
                                style: "
                                    margin-top: 24px;
                                    padding: 20px;
                                    background: #ffffff;
                                    border: 1px solid #e5e7eb;
                                    border-radius: 12px;
                                    text-align: center;
                                    color: #6b7280;
                                ",
                                "No notes match the current filters."
                            }
                        } else {
                            for note in filtered_notes {
                                {
                                    let note_id = note.id;
                                    let note_content = note.content.clone();
                                    let title = note_title(&note);
                                    let preview = note_preview(&note);
                                    let updated = relative_time(note.updated_at);
                                    let selected = selected_note_id() == Some(note_id);
                                    let border_color = if selected { "#2563eb" } else { "#e5e7eb" };
                                    let card_style = format!(
                                        "margin-bottom: 10px;\
                                         width: 100%;\
                                         border: 1px solid {border_color};\
                                         background: #ffffff;\
                                         border-radius: 12px;\
                                         padding: 12px;\
                                         text-align: left;"
                                    );

                                    rsx! {
                                        UiButton {
                                            key: "{note_id}",
                                            type: "button",
                                            variant: ButtonVariant::Ghost,
                                            style: "{card_style}",
                                            onclick: move |_| {
                                                selected_note_id.set(Some(note_id));
                                                draft_content.set(note_content.clone());
                                                status_message.set(None);
                                                attachment_upload_error.set(None);
                                                attachment_preview_open.set(false);
                                                view.set(MobileView::Editor);
                                            },

                                            p {
                                                style: "
                                                    margin: 0 0 6px 0;
                                                    font-size: 15px;
                                                    font-weight: 600;
                                                    color: #111827;
                                                ",
                                                "{title}"
                                            }
                                            p {
                                                style: "
                                                    margin: 0 0 6px 0;
                                                    font-size: 13px;
                                                    color: #6b7280;
                                                ",
                                                "{preview}"
                                            }
                                            p {
                                                style: "margin: 0; font-size: 12px; color: #9ca3af;",
                                                "Updated {updated}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else if view() == MobileView::Settings {
                ScrollArea {
                    direction: ScrollDirection::Vertical,
                    scroll_type: ScrollType::Auto,
                    tabindex: "0",
                    style: "flex: 1; padding: 12px;",

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 6px;
                            margin-bottom: 10px;
                        ",
                        p {
                            style: "
                                margin: 0;
                                font-size: 12px;
                                font-weight: 700;
                                color: #6b7280;
                                text-transform: uppercase;
                                letter-spacing: 0.04em;
                            ",
                            "Sync"
                        }
                        p {
                            style: "margin: 0; font-size: 14px; color: #111827;",
                            "{sync_state_text}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Last successful sync: {last_sync_text}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Scheduler: {sync_scheduler_text}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Last scheduler attempt: {last_sync_attempt_text}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Consecutive sync failures: {consecutive_sync_failures}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Pending local changes: {pending_sync_count_value}"
                        }
                        if pending_sync_count_value > 0 {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "Pending note IDs: {pending_sync_preview}"
                            }
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            if diagnostics.turso_sync_configured {
                                "Mode: remote sync configured"
                            } else {
                                "Mode: local-only (no Turso sync config)"
                            }
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Config source: {diagnostics.turso_active_source}"
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
                        ",
                        div {
                            style: "display: flex; align-items: center; justify-content: space-between; gap: 8px;",
                            p {
                                style: "
                                    margin: 0;
                                    font-size: 12px;
                                    font-weight: 700;
                                    color: #6b7280;
                                    text-transform: uppercase;
                                    letter-spacing: 0.04em;
                                ",
                                "Sync conflicts"
                            }
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Outline,
                                style: "padding: 6px 10px; font-size: 12px;",
                                disabled: sync_conflicts_loading(),
                                onclick: on_refresh_sync_conflicts,
                                "Refresh"
                            }
                        }

                        if sync_conflicts_loading() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "Loading recent conflicts..."
                            }
                        } else if let Some(error) = sync_conflicts_error() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #b91c1c;",
                                "{error}"
                            }
                        } else if sync_conflicts().is_empty() {
                            p {
                                style: "margin: 0; font-size: 12px; color: #6b7280;",
                                "No sync conflicts recorded yet."
                            }
                        } else {
                            div {
                                style: "display: flex; flex-direction: column; gap: 8px;",
                                for conflict in sync_conflicts() {
                                    div {
                                        key: "{conflict.id}",
                                        style: "padding: 8px; border: 1px solid #e5e7eb; border-radius: 8px; display: flex; flex-direction: column; gap: 3px;",
                                        p {
                                            style: "margin: 0; font-size: 12px; color: #111827;",
                                            "Note {conflict.note_id}"
                                        }
                                        p {
                                            style: "margin: 0; font-size: 11px; color: #6b7280;",
                                            "Resolved: {format_sync_conflict_time(conflict.resolved_at)}"
                                        }
                                        p {
                                            style: "margin: 0; font-size: 11px; color: #6b7280;",
                                            "Local ts: {conflict.local_updated_at}, incoming ts: {conflict.incoming_updated_at}, strategy: {conflict.strategy}"
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
                        ",
                        p {
                            style: "
                                margin: 0;
                                font-size: 12px;
                                font-weight: 700;
                                color: #6b7280;
                                text-transform: uppercase;
                                letter-spacing: 0.04em;
                            ",
                            "Authentication"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Session: {auth_session_summary}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Auth config: {auth_config_summary_text}"
                        }
                        Label {
                            html_for: "auth-email",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Email"
                        }
                        UiInput {
                            id: "auth-email",
                            r#type: "email",
                            placeholder: "Email",
                            value: "{auth_email_input}",
                            oninput: move |event: Event<FormData>| {
                                auth_email_input.set(event.value());
                            },
                        }
                        Label {
                            html_for: "auth-password",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Password"
                        }
                        UiInput {
                            id: "auth-password",
                            r#type: "password",
                            placeholder: "Password",
                            value: "{auth_password_input}",
                            oninput: move |event: Event<FormData>| {
                                auth_password_input.set(event.value());
                            },
                        }
                        div {
                            style: "display: flex; gap: 8px; flex-wrap: wrap;",
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Primary,
                                style: "flex: 1; min-width: 100px;",
                                disabled: auth_loading(),
                                onclick: on_auth_sign_in,
                                if auth_loading() { "Working..." } else { "Sign in" }
                            }
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Outline,
                                style: "flex: 1; min-width: 100px;",
                                disabled: auth_loading(),
                                onclick: on_auth_sign_up,
                                "Sign up"
                            }
                            UiButton {
                                type: "button",
                                variant: ButtonVariant::Outline,
                                style: "flex: 1; min-width: 100px;",
                                disabled: auth_loading() || auth_session().is_none(),
                                onclick: on_auth_sign_out,
                                "Sign out"
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
                        ",
                        p {
                            style: "
                                margin: 0;
                                font-size: 12px;
                                font-weight: 700;
                                color: #6b7280;
                                text-transform: uppercase;
                                letter-spacing: 0.04em;
                            ",
                            "Export"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Destination: {export_directory_text}"
                        }
                        div {
                            style: "display: flex; gap: 8px;",
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Outline,
                                disabled: export_busy(),
                                onclick: on_export_json,
                                if export_busy() { "Exporting..." } else { "Export JSON" }
                            }
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Outline,
                                disabled: export_busy(),
                                onclick: on_export_markdown,
                                "Export Markdown"
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 8px;
                            margin-bottom: 10px;
                        ",
                        p {
                            style: "
                                margin: 0;
                                font-size: 12px;
                                font-weight: 700;
                                color: #6b7280;
                                text-transform: uppercase;
                                letter-spacing: 0.04em;
                            ",
                            "Turso sync settings"
                        }
                        Label {
                            html_for: "turso-url",
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Turso URL"
                        }
                        UiInput {
                            id: "turso-url",
                            r#type: "text",
                            placeholder: "libsql://your-db.region.turso.io",
                            value: "{turso_database_url_input}",
                            oninput: move |event: Event<FormData>| {
                                turso_database_url_input.set(event.value());
                            },
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Managed mode uses signed-in Supabase session + TURSO_SYNC_TOKEN_ENDPOINT to fetch short-lived sync credentials."
                        }
                        div {
                            style: "display: flex; gap: 8px;",
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Primary,
                                onclick: on_save_sync_settings,
                                "Save sync config"
                            }
                            UiButton {
                                type: "button",
                                block: true,
                                variant: ButtonVariant::Outline,
                                onclick: on_clear_sync_settings,
                                "Clear"
                            }
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 6px;
                            margin-bottom: 10px;
                        ",
                        p {
                            style: "
                                margin: 0;
                                font-size: 12px;
                                font-weight: 700;
                                color: #6b7280;
                                text-transform: uppercase;
                                letter-spacing: 0.04em;
                            ",
                            "Build"
                        }
                        p {
                            style: "margin: 0; font-size: 13px; color: #111827;",
                            "{package_name} v{app_version}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Target: {std::env::consts::ARCH}/{std::env::consts::OS}"
                        }
                    }

                    div {
                        style: "
                            padding: 12px;
                            border: 1px solid #e5e7eb;
                            border-radius: 12px;
                            background: #ffffff;
                            display: flex;
                            flex-direction: column;
                            gap: 6px;
                        ",
                        p {
                            style: "
                                margin: 0;
                                font-size: 12px;
                                font-weight: 700;
                                color: #6b7280;
                                text-transform: uppercase;
                                letter-spacing: 0.04em;
                            ",
                            "Configuration diagnostics"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Turso runtime endpoint: {diagnostics.turso_runtime_endpoint}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Turso runtime token: {diagnostics.turso_runtime_token_status}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Managed token endpoint: {diagnostics.turso_managed_auth_endpoint}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Turso env endpoint: {diagnostics.turso_env_endpoint}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Turso env token: {diagnostics.turso_env_token_status}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Supabase URL: {diagnostics.supabase_url}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Supabase anon key: {diagnostics.supabase_anon_key_status}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "Supabase auth config: {diagnostics.supabase_auth_status}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "R2 bucket: {diagnostics.r2_bucket}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "R2 endpoint: {diagnostics.r2_endpoint}"
                        }
                        p {
                            style: "margin: 0; font-size: 12px; color: #374151;",
                            "R2 credentials: {diagnostics.r2_credentials_status}"
                        }
                    }
                }
            } else {
                div {
                    style: "
                        padding: 10px 12px;
                        display: flex;
                        gap: 8px;
                        background: #ffffff;
                    ",
                    UiButton {
                        type: "button",
                        variant: ButtonVariant::Outline,
                        onclick: on_back_to_list,
                        "Back"
                    }
                    UiButton {
                        type: "button",
                        variant: ButtonVariant::Primary,
                        disabled: saving(),
                        onclick: on_save_note,
                        if saving() { "Saving..." } else { "Save" }
                    }
                    if selected_note_id().is_some() {
                        UiButton {
                            type: "button",
                            variant: ButtonVariant::Danger,
                            style: "margin-left: auto;",
                            disabled: deleting(),
                            onclick: on_delete_note,
                            if deleting() { "Deleting..." } else { "Delete" }
                        }
                    }
                }

                Separator {
                    decorative: true,
                    style: "height: 1px; background: #e5e7eb;",
                }

                UiTextarea {
                    style: "
                        flex: 1;
                        margin: 12px;
                        border-radius: 12px;
                        padding: 14px;
                        line-height: 1.5;
                        font-size: 15px;
                    ",
                    value: "{draft_content}",
                    placeholder: "Write your note...",
                    oninput: move |event: Event<FormData>| {
                        draft_content.set(event.value());
                    },
                }

                div {
                    style: "
                        margin: 0 12px 12px 12px;
                        padding: 10px;
                        border: 1px solid #e5e7eb;
                        border-radius: 10px;
                        background: #ffffff;
                        display: flex;
                        flex-direction: column;
                        gap: 6px;
                    ",
                    p {
                        style: "
                            margin: 0;
                            font-size: 12px;
                            font-weight: 700;
                            color: #6b7280;
                            text-transform: uppercase;
                            letter-spacing: 0.04em;
                        ",
                        "Attachments"
                    }
                    if selected_note_id().is_some() {
                        UiInput {
                            id: "attachment-file-input",
                            r#type: "file",
                            disabled: attachment_uploading(),
                            onchange: on_pick_attachment,
                        }
                    }

                    if attachment_uploading() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Uploading attachment..."
                        }
                    }
                    if let Some(error) = attachment_upload_error() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #b91c1c;",
                            "{error}"
                        }
                    }

                    if selected_note_id().is_none() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Save this note to view attachments."
                        }
                    } else if attachments_loading() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "Loading attachments..."
                        }
                    } else if let Some(error) = attachments_error() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #b91c1c;",
                            "{error}"
                        }
                    } else if note_attachments().is_empty() {
                        p {
                            style: "margin: 0; font-size: 12px; color: #6b7280;",
                            "No attachments yet."
                        }
                    } else {
                        for attachment in note_attachments() {
                            div {
                                key: "{attachment.id}",
                                style: "
                                    display: flex;
                                    flex-direction: column;
                                    gap: 8px;
                                    font-size: 12px;
                                    padding: 8px;
                                    border: 1px solid #e5e7eb;
                                    border-radius: 8px;
                                ",
                                div {
                                    style: "display: flex; justify-content: space-between; align-items: center; gap: 8px;",
                                    p {
                                        style: "
                                            margin: 0;
                                            color: #111827;
                                            min-width: 0;
                                            flex: 1;
                                            overflow: hidden;
                                            text-overflow: ellipsis;
                                            white-space: nowrap;
                                        ",
                                        "{attachment.filename}"
                                    }
                                    p {
                                        style: "margin: 0; color: #6b7280; white-space: nowrap;",
                                        "{attachment_kind_label(&attachment.filename, &attachment.mime_type)}"
                                    }
                                    p {
                                        style: "margin: 0; color: #6b7280; white-space: nowrap;",
                                        "{format_attachment_size(attachment.size_bytes)}"
                                    }
                                }
                                div {
                                    style: "display: flex; gap: 8px;",
                                    {
                                        let attachment_id = attachment.id;
                                        let attachment_for_preview = attachment.clone();
                                        let attachment_for_delete = attachment.clone();
                                        let deleting_now = deleting_attachment_id() == Some(attachment_id);

                                        rsx! {
                                            UiButton {
                                                type: "button",
                                                variant: ButtonVariant::Outline,
                                                style: "padding: 6px 10px; font-size: 12px;",
                                                disabled: deleting_now,
                                                onclick: move |_| {
                                                    let attachment_for_preview =
                                                        attachment_for_preview.clone();
                                                    attachment_preview_open.set(true);
                                                    attachment_preview_loading.set(true);
                                                    attachment_preview_error.set(None);
                                                    attachment_preview_content.set(AttachmentPreview::None);
                                                    attachment_preview_title.set(attachment_for_preview.filename.clone());

                                                    spawn(async move {
                                                        match load_attachment_preview_from_r2(&attachment_for_preview).await {
                                                            Ok(preview) => attachment_preview_content.set(preview),
                                                            Err(error) => attachment_preview_error.set(Some(error)),
                                                        }
                                                        attachment_preview_loading.set(false);
                                                    });
                                                },
                                                "Open"
                                            }
                                            UiButton {
                                                type: "button",
                                                variant: ButtonVariant::Danger,
                                                style: "padding: 6px 10px; font-size: 12px;",
                                                disabled: deleting_now,
                                                onclick: move |_| {
                                                    let attachment_for_delete =
                                                        attachment_for_delete.clone();
                                                    let Some(note_store) = store.read().clone() else {
                                                        attachments_error.set(Some("Database is not ready yet.".to_string()));
                                                        return;
                                                    };

                                                    deleting_attachment_id.set(Some(attachment_id));
                                                    attachments_error.set(None);

                                                    spawn(async move {
                                                        match note_store.delete_attachment(&attachment_id).await {
                                                            Ok(()) => {
                                                                enqueue_pending_sync_change(
                                                                    attachment_for_delete.note_id,
                                                                    &mut pending_sync_note_ids,
                                                                    &mut pending_sync_count,
                                                                );
                                                                if let Err(error) = delete_attachment_object_from_r2(&attachment_for_delete.r2_key).await {
                                                                    attachments_error.set(Some(format!(
                                                                        "Attachment removed, but failed to delete remote object: {error}"
                                                                    )));
                                                                }
                                                                attachment_refresh_version.set(attachment_refresh_version() + 1);
                                                                status_message.set(Some("Attachment deleted.".to_string()));
                                                            }
                                                            Err(error) => {
                                                                attachments_error.set(Some(format!(
                                                                    "Failed to delete attachment: {error}"
                                                                )));
                                                            }
                                                        }
                                                        deleting_attachment_id.set(None);
                                                    });
                                                },
                                                if deleting_now { "Deleting..." } else { "Delete" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if attachment_preview_open() {
                    div {
                        style: "
                            position: fixed;
                            inset: 0;
                            background: rgba(17, 24, 39, 0.55);
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            padding: 16px;
                            z-index: 9998;
                        ",
                        div {
                            style: "
                                width: 100%;
                                max-width: 520px;
                                max-height: 80vh;
                                background: #ffffff;
                                border-radius: 12px;
                                border: 1px solid #e5e7eb;
                                display: flex;
                                flex-direction: column;
                            ",
                            div {
                                style: "display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 12px; border-bottom: 1px solid #e5e7eb;",
                                p {
                                    style: "margin: 0; font-size: 14px; font-weight: 600; color: #111827; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                    "{attachment_preview_title()}"
                                }
                                UiButton {
                                    type: "button",
                                    variant: ButtonVariant::Outline,
                                    style: "padding: 6px 10px; font-size: 12px;",
                                    onclick: on_close_attachment_preview,
                                    "Close"
                                }
                            }
                            div {
                                style: "padding: 12px; overflow: auto;",
                                if attachment_preview_loading() {
                                    p {
                                        style: "margin: 0; font-size: 12px; color: #6b7280;",
                                        "Loading preview..."
                                    }
                                } else if let Some(error) = attachment_preview_error() {
                                    p {
                                        style: "margin: 0; font-size: 12px; color: #b91c1c;",
                                        "{error}"
                                    }
                                } else {
                                    {render_attachment_preview(attachment_preview_content(), &attachment_preview_title())}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn apply_quick_capture_launch(seed_text: Option<String>, draft_content: &mut Signal<String>) {
    draft_content.set(seed_text.unwrap_or_default());
}

fn apply_share_intent(
    shared_text: String,
    selected_note_id: &mut Signal<Option<NoteId>>,
    draft_content: &mut Signal<String>,
    status_message: &mut Signal<Option<String>>,
) {
    selected_note_id.set(None);
    draft_content.set(shared_text);
    status_message.set(Some("Shared text ready to save".to_string()));
}

fn resolve_r2_storage() -> Result<R2Storage, String> {
    match R2Config::from_env() {
        Ok(Some(config)) => Ok(R2Storage::new(config)),
        Ok(None) => Err("R2 is not configured. Set R2 env vars first.".to_string()),
        Err(error) => Err(format!("Invalid R2 configuration: {error}")),
    }
}

async fn upload_attachment_to_r2(
    note_store: Arc<MobileNoteStore>,
    note_id: NoteId,
    file_name: String,
    content_type: Option<String>,
    file_bytes: Vec<u8>,
) -> Result<(), String> {
    let storage = resolve_r2_storage()?;
    let object_key = storage
        .build_media_key(&note_id.to_string(), &file_name)
        .map_err(|error| format!("Failed to build media key: {error}"))?;
    let mime_type = infer_attachment_mime_type(content_type.as_deref(), &file_name);

    storage
        .upload_bytes(&object_key, file_bytes.as_ref(), Some(&mime_type))
        .await
        .map_err(|error| format!("Failed to upload attachment to R2: {error}"))?;

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
) -> Result<AttachmentPreview, String> {
    let storage = resolve_r2_storage()?;
    let (bytes, downloaded_content_type) = storage
        .download_bytes(&attachment.r2_key)
        .await
        .map_err(|error| format!("Failed to download attachment: {error}"))?;

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

async fn delete_attachment_object_from_r2(object_key: &str) -> Result<(), String> {
    let storage = resolve_r2_storage()?;
    storage
        .delete_object(object_key)
        .await
        .map_err(|error| format!("{error}"))
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
) -> MobileConfigDiagnostics {
    let runtime_config = load_runtime_config();
    let turso_runtime_url = runtime_config.turso_database_url;
    let turso_runtime_token_status = runtime_turso_token_status();
    let managed_sync_endpoint = env_var_trimmed("TURSO_SYNC_TOKEN_ENDPOINT");

    let turso_env_url = env_var_trimmed("TURSO_DATABASE_URL");
    let turso_env_token_set = env_var_trimmed("TURSO_AUTH_TOKEN").is_some();
    let supabase_url = env_var_trimmed("SUPABASE_URL");
    let supabase_anon_key_set = env_var_trimmed("SUPABASE_ANON_KEY").is_some();

    let r2_account_id = env_var_trimmed("R2_ACCOUNT_ID");
    let r2_bucket = env_var_trimmed("R2_BUCKET");
    let r2_access_key_set = env_var_trimmed("R2_ACCESS_KEY_ID").is_some();
    let r2_secret_key_set = env_var_trimmed("R2_SECRET_ACCESS_KEY").is_some();

    let r2_endpoint = r2_account_id
        .as_deref()
        .map(|account_id| format!("https://{account_id}.r2.cloudflarestorage.com"));

    MobileConfigDiagnostics {
        turso_sync_configured: !matches!(active_sync_source, SyncConfigSource::None),
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
        turso_env_endpoint: turso_env_url
            .as_deref()
            .map(mask_endpoint_value)
            .unwrap_or_else(|| "not set".to_string()),
        turso_env_token_status: configured_status_label(turso_env_token_set).to_string(),
        supabase_url: supabase_url
            .as_deref()
            .map(mask_endpoint_value)
            .unwrap_or_else(|| "not set".to_string()),
        supabase_anon_key_status: configured_status_label(supabase_anon_key_set).to_string(),
        supabase_auth_status: auth_config
            .map(auth_config_summary)
            .unwrap_or_else(|| "unknown".to_string()),
        r2_bucket: r2_bucket.unwrap_or_else(|| "not set".to_string()),
        r2_endpoint: r2_endpoint.unwrap_or_else(|| "not set".to_string()),
        r2_credentials_status: configured_status_label(r2_access_key_set && r2_secret_key_set)
            .to_string(),
    }
}

fn sync_config_source_label(source: SyncConfigSource) -> &'static str {
    match source {
        SyncConfigSource::RuntimeSettings => "runtime settings",
        SyncConfigSource::EnvironmentFallback => "env fallback",
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

fn env_var_trimmed(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
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

    #[test]
    fn formats_attachment_sizes_for_mobile_ui() {
        assert_eq!(format_attachment_size(800), "800 B");
        assert_eq!(format_attachment_size(1_536), "1.5 KB");
        assert_eq!(format_attachment_size(3_145_728), "3.0 MB");
        assert_eq!(format_attachment_size(-1), "0 B");
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
}
