use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use dioxus_primitives::scroll_area::{ScrollArea, ScrollDirection, ScrollType};
use dioxus_primitives::separator::Separator;
use dioxus_primitives::toast::{use_toast, ToastOptions, ToastProvider};
use dirt_core::{Attachment, Note, NoteId};

use crate::config::{
    load_runtime_config, resolve_sync_config, save_runtime_config, MobileRuntimeConfig,
    SyncConfigSource,
};
use crate::data::MobileNoteStore;
use crate::launch::LaunchIntent;

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
    turso_runtime_endpoint: String,
    turso_runtime_token_status: String,
    turso_env_endpoint: String,
    turso_env_token_status: String,
    supabase_url: String,
    supabase_anon_key_status: String,
    r2_bucket: String,
    r2_endpoint: String,
    r2_credentials_status: String,
}

const KIB_BYTES: u64 = 1024;
const MIB_BYTES: u64 = KIB_BYTES * 1024;
const GIB_BYTES: u64 = MIB_BYTES * 1024;
const SYNC_INTERVAL_SECS: u64 = 30;
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
    let mut note_attachments = use_signal(Vec::<Attachment>::new);
    let mut attachments_loading = use_signal(|| false);
    let mut attachments_error = use_signal(|| None::<String>);
    let attachment_refresh_version = use_signal(|| 0u64);
    let mut db_init_retry_version = use_signal(|| 0u64);
    let mut turso_database_url_input = use_signal(String::new);
    let mut turso_auth_token_input = use_signal(String::new);
    let mut active_sync_source = use_signal(|| SyncConfigSource::None);
    let launch: Signal<LaunchIntent> = use_signal(crate::launch::detect_launch_intent_from_runtime);
    let mut launch_applied = use_signal(|| false);
    let toasts = use_toast();

    use_future(move || async move {
        let _db_init_retry_version = db_init_retry_version();
        let runtime_config = load_runtime_config();
        turso_database_url_input.set(runtime_config.turso_database_url.unwrap_or_default());
        turso_auth_token_input.set(runtime_config.turso_auth_token.unwrap_or_default());
        active_sync_source.set(resolve_sync_config().source);

        loading.set(true);
        store.set(None);
        notes.set(Vec::new());
        sync_state.set(MobileSyncState::Offline);
        last_sync_at.set(None);
        sync_scheduler_active.set(false);
        last_sync_attempt_at.set(None);
        consecutive_sync_failures.set(0);
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
                    status_message.set(Some(
                        "Running in local-only mode (set Turso URL + auth token in Settings to enable auto-sync)"
                            .to_string(),
                    ));
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
    };

    let on_open_settings = move |_| {
        view.set(MobileView::Settings);
    };

    let on_save_sync_settings = move |_| {
        let runtime_config = MobileRuntimeConfig::from_raw(
            Some(turso_database_url_input()),
            Some(turso_auth_token_input()),
        );

        if !runtime_config.has_sync_config() {
            status_message.set(Some(
                "Both Turso URL and auth token are required to enable remote sync".to_string(),
            ));
            return;
        }

        match save_runtime_config(&runtime_config) {
            Ok(()) => {
                active_sync_source.set(SyncConfigSource::RuntimeSettings);
                status_message.set(Some(
                    "Saved sync settings. Reinitializing database connection...".to_string(),
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
        match save_runtime_config(&empty_config) {
            Ok(()) => {
                turso_database_url_input.set(String::new());
                turso_auth_token_input.set(String::new());
                active_sync_source.set(SyncConfigSource::None);
                status_message.set(Some(
                    "Cleared runtime sync settings. Reinitializing database connection..."
                        .to_string(),
                ));
                db_init_retry_version.set(db_init_retry_version() + 1);
            }
            Err(error) => {
                status_message.set(Some(format!("Failed to clear sync settings: {error}")));
            }
        }
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

    let diagnostics = mobile_config_diagnostics(active_sync_source());
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
    let app_version = env!("CARGO_PKG_VERSION");
    let package_name = env!("CARGO_PKG_NAME");

    rsx! {
        style {
            "{TOAST_STYLES}"
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
                        button {
                            type: "button",
                            style: "
                                border: 1px solid #d1d5db;
                                border-radius: 8px;
                                padding: 6px 10px;
                                background: #ffffff;
                                color: #111827;
                                font-size: 12px;
                                font-weight: 600;
                            ",
                            onclick: on_back_to_list,
                            "Notes"
                        }
                    } else {
                        button {
                            type: "button",
                            style: "
                                border: 1px solid #d1d5db;
                                border-radius: 8px;
                                padding: 6px 10px;
                                background: #ffffff;
                                color: #111827;
                                font-size: 12px;
                                font-weight: 600;
                            ",
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
                            button {
                                type: "button",
                                style: "
                                    border: 0;
                                    border-radius: 8px;
                                    padding: 10px 12px;
                                    background: #2563eb;
                                    color: #ffffff;
                                    font-weight: 600;
                                ",
                                onclick: on_retry_db_init,
                                disabled: loading(),
                                "Retry"
                            }
                        }
                    }
                } else {
                    div {
                        style: "padding: 12px 16px; display: flex; gap: 8px;",
                        button {
                            type: "button",
                            style: "
                                flex: 1;
                                border: 0;
                                border-radius: 10px;
                                padding: 12px;
                                background: #111827;
                                color: #ffffff;
                                font-weight: 600;
                                font-size: 14px;
                            ",
                            onclick: on_new_note,
                            "New note"
                        }
                    }

                    ScrollArea {
                        direction: ScrollDirection::Vertical,
                        scroll_type: ScrollType::Auto,
                        tabindex: "0",
                        style: "flex: 1; padding: 0 12px 16px 12px;",

                        if notes().is_empty() {
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
                        } else {
                            for note in notes() {
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
                                        button {
                                            key: "{note_id}",
                                            type: "button",
                                            style: "{card_style}",
                                            onclick: move |_| {
                                                selected_note_id.set(Some(note_id));
                                                draft_content.set(note_content.clone());
                                                status_message.set(None);
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
                        input {
                            r#type: "text",
                            placeholder: "libsql://your-db.region.turso.io",
                            value: "{turso_database_url_input}",
                            style: "
                                border: 1px solid #d1d5db;
                                border-radius: 8px;
                                padding: 10px;
                                font-size: 13px;
                            ",
                            oninput: move |event: Event<FormData>| {
                                turso_database_url_input.set(event.value());
                            },
                        }
                        input {
                            r#type: "password",
                            placeholder: "Turso auth token",
                            value: "{turso_auth_token_input}",
                            style: "
                                border: 1px solid #d1d5db;
                                border-radius: 8px;
                                padding: 10px;
                                font-size: 13px;
                            ",
                            oninput: move |event: Event<FormData>| {
                                turso_auth_token_input.set(event.value());
                            },
                        }
                        div {
                            style: "display: flex; gap: 8px;",
                            button {
                                type: "button",
                                style: "
                                    flex: 1;
                                    border: 0;
                                    border-radius: 8px;
                                    padding: 10px;
                                    background: #2563eb;
                                    color: #ffffff;
                                    font-weight: 600;
                                    font-size: 13px;
                                ",
                                onclick: on_save_sync_settings,
                                "Save sync config"
                            }
                            button {
                                type: "button",
                                style: "
                                    flex: 1;
                                    border: 1px solid #d1d5db;
                                    border-radius: 8px;
                                    padding: 10px;
                                    background: #ffffff;
                                    color: #374151;
                                    font-weight: 600;
                                    font-size: 13px;
                                ",
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
                    button {
                        type: "button",
                        style: "
                            border: 1px solid #d1d5db;
                            border-radius: 8px;
                            padding: 10px 12px;
                            background: #ffffff;
                            font-weight: 600;
                        ",
                        onclick: on_back_to_list,
                        "Back"
                    }
                    button {
                        type: "button",
                        style: "
                            border: 0;
                            border-radius: 8px;
                            padding: 10px 12px;
                            background: #2563eb;
                            color: #ffffff;
                            font-weight: 600;
                        ",
                        disabled: saving(),
                        onclick: on_save_note,
                        if saving() { "Saving..." } else { "Save" }
                    }
                    if selected_note_id().is_some() {
                        button {
                            type: "button",
                            style: "
                                margin-left: auto;
                                border: 1px solid #ef4444;
                                border-radius: 8px;
                                padding: 10px 12px;
                                background: #ffffff;
                                color: #b91c1c;
                                font-weight: 600;
                            ",
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

                textarea {
                    style: "
                        flex: 1;
                        margin: 12px;
                        border: 1px solid #d1d5db;
                        border-radius: 12px;
                        padding: 14px;
                        line-height: 1.5;
                        font-size: 15px;
                        resize: none;
                        background: #ffffff;
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
                                    justify-content: space-between;
                                    align-items: center;
                                    gap: 8px;
                                    font-size: 12px;
                                ",
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
                                    "{attachment_kind_label(&attachment.mime_type)}"
                                }
                                p {
                                    style: "margin: 0; color: #6b7280; white-space: nowrap;",
                                    "{format_attachment_size(attachment.size_bytes)}"
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

fn mobile_config_diagnostics(active_sync_source: SyncConfigSource) -> MobileConfigDiagnostics {
    let runtime_config = load_runtime_config();
    let turso_runtime_url = runtime_config.turso_database_url;
    let turso_runtime_token_set = runtime_config
        .turso_auth_token
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());

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
        turso_runtime_endpoint: turso_runtime_url
            .as_deref()
            .map(mask_endpoint_value)
            .unwrap_or_else(|| "not set".to_string()),
        turso_runtime_token_status: configured_status_label(turso_runtime_token_set).to_string(),
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

fn attachment_kind_label(mime_type: &str) -> &'static str {
    if mime_type.starts_with("image/") {
        "image"
    } else if mime_type.starts_with("audio/") {
        "audio"
    } else if mime_type.starts_with("video/") {
        "video"
    } else if mime_type.starts_with("text/") {
        "text"
    } else {
        "file"
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
        assert_eq!(attachment_kind_label("image/png"), "image");
        assert_eq!(attachment_kind_label("audio/wav"), "audio");
        assert_eq!(attachment_kind_label("video/mp4"), "video");
        assert_eq!(attachment_kind_label("text/plain"), "text");
        assert_eq!(attachment_kind_label("application/pdf"), "file");
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
}
