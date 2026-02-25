//! Settings panel component

use std::sync::Arc;

use dioxus::prelude::*;
use dioxus_primitives::slider::SliderValue;
use rfd::AsyncFileDialog;

use dirt_core::models::{NoteId, Settings, SyncConflict, ThemeMode};

use super::button::{Button, ButtonVariant};
use super::dialog::{DialogContent, DialogRoot, DialogTitle};
use super::input::Input;
use super::select::{
    Select, SelectItemIndicator, SelectList, SelectOption, SelectTrigger, SelectValue,
};
use super::slider::{Slider, SliderRange, SliderThumb, SliderTrack};
use crate::services::{
    export_notes_to_path, suggested_export_file_name, AuthConfigStatus, NotesExportFormat,
    SignUpOutcome, TranscriptionConfigStatus, TranscriptionService,
};
use crate::state::AppState;
use crate::theme::resolve_theme;

/// Font family options
const FONT_FAMILIES: &[(&str, &str)] = &[
    ("system-ui", "System Default"),
    ("JetBrains Mono", "JetBrains Mono"),
    ("Fira Code", "Fira Code"),
    ("Consolas", "Consolas"),
    ("Monaco", "Monaco"),
    ("Menlo", "Menlo"),
];
const SYNC_CONFLICT_LIMIT: usize = 10;

/// Settings panel component
#[component]
pub fn SettingsPanel() -> Element {
    let state = use_context::<AppState>();
    let mut settings = state.settings;
    let mut theme = state.theme;
    let mut settings_open = state.settings_open;
    let db_service = state.db_service;

    let colors = (state.theme)().palette();

    // Save settings to database
    let save_settings = move |new_settings: Settings| {
        // Update theme resolution when theme mode changes
        let resolved = resolve_theme(new_settings.theme);
        theme.set(resolved);
        settings.set(new_settings.clone());

        // Save to database asynchronously
        let db = db_service.read().clone();
        spawn(async move {
            if let Some(db) = db {
                if let Err(e) = db.save_settings(&new_settings).await {
                    tracing::error!("Failed to save settings: {}", e);
                }
            }
        });
    };

    let close_settings = move |_: MouseEvent| {
        settings_open.set(false);
    };

    let current_settings = settings();
    let current_theme_value = match current_settings.theme {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
    };
    let auth_service = state.auth_service.read().clone();
    let mut transcription_service_signal = state.transcription_service;
    let transcription_service = transcription_service_signal.read().clone();
    let transcription_config_status = transcription_service
        .as_ref()
        .map(|service| service.config_status());
    let transcription_available = transcription_config_status
        .as_ref()
        .is_some_and(|status| status.enabled);
    let transcription_status_text = transcription_status_text(
        transcription_config_status.as_ref(),
        current_settings.voice_memo_transcription_enabled,
    );
    let transcription_toggle_disabled =
        !transcription_available && !current_settings.voice_memo_transcription_enabled;
    let mut openai_api_key_input = use_signal(String::new);
    let mut openai_api_key_message = use_signal(|| None::<String>);
    let mut openai_api_key_configured = use_signal(|| {
        TranscriptionService::has_stored_api_key().unwrap_or_else(|error| {
            tracing::warn!("Failed to check stored OpenAI API key: {}", error);
            false
        })
    });
    let active_session = (state.auth_session)();
    let pending_sync_count = (state.pending_sync_count)();
    let pending_sync_note_ids = (state.pending_sync_note_ids)();
    let pending_sync_preview = format_pending_sync_preview(&pending_sync_note_ids);
    let init_auth_error = (state.auth_error)();
    let signed_in_identity = active_session.as_ref().map(|session| {
        session
            .user
            .email
            .clone()
            .unwrap_or_else(|| session.user.id.clone())
    });
    let mut auth_email = use_signal(String::new);
    let mut auth_password = use_signal(String::new);
    let mut auth_message = use_signal(|| None::<String>);
    let mut auth_busy = use_signal(|| false);
    let mut auth_verifying = use_signal(|| false);
    let auth_config_status = use_signal(|| None::<AuthConfigStatus>);
    let mut auth_config_checked = use_signal(|| false);
    let auth_service_for_preflight = auth_service.clone();
    let mut export_busy = use_signal(|| false);
    let mut export_message = use_signal(|| None::<String>);
    let sync_conflicts = use_signal(Vec::<SyncConflict>::new);
    let mut sync_conflicts_loading = use_signal(|| false);
    let mut sync_conflicts_error = use_signal(|| None::<String>);
    let mut sync_conflicts_refresh_version = use_signal(|| 0u64);

    use_effect(move || {
        if auth_config_checked() || auth_service_for_preflight.is_none() {
            return;
        }

        auth_config_checked.set(true);
        auth_verifying.set(true);

        let mut auth_error_signal = state.auth_error;
        let mut auth_verifying_signal = auth_verifying;
        let mut auth_config_status_signal = auth_config_status;
        let service = auth_service_for_preflight.clone();

        spawn(async move {
            let Some(service) = service else {
                auth_verifying_signal.set(false);
                return;
            };

            match service.verify_configuration().await {
                Ok(status) => {
                    auth_error_signal.set(None);
                    auth_config_status_signal.set(Some(status));
                }
                Err(error) => {
                    tracing::error!("Auth preflight verify failed: {}", error);
                    auth_error_signal.set(Some(format_auth_error_message(&error.to_string())));
                    auth_config_status_signal.set(None);
                }
            }

            auth_verifying_signal.set(false);
        });
    });

    use_effect(move || {
        let _refresh_version = sync_conflicts_refresh_version();
        let db = state.db_service.read().clone();

        sync_conflicts_loading.set(true);
        sync_conflicts_error.set(None);

        let mut conflicts_signal = sync_conflicts;
        let mut loading_signal = sync_conflicts_loading;
        let mut error_signal = sync_conflicts_error;
        spawn(async move {
            let Some(db) = db else {
                conflicts_signal.set(Vec::new());
                loading_signal.set(false);
                error_signal.set(Some("Database service is not available.".to_string()));
                return;
            };

            match db.list_conflicts(SYNC_CONFLICT_LIMIT).await {
                Ok(conflicts) => {
                    conflicts_signal.set(conflicts);
                }
                Err(error) => {
                    conflicts_signal.set(Vec::new());
                    error_signal.set(Some(format!("Failed to load sync conflicts: {error}")));
                }
            }

            loading_signal.set(false);
        });
    });

    let sign_in = move |_: MouseEvent| {
        let Some(service) = state.auth_service.read().clone() else {
            auth_message.set(Some(
                "Authentication is not available in this build.".to_string(),
            ));
            return;
        };
        let email = auth_email().trim().to_string();
        let password = auth_password();
        if email.is_empty() || password.trim().is_empty() {
            auth_message.set(Some("Email and password are required.".to_string()));
            return;
        }

        auth_busy.set(true);
        auth_message.set(None);

        let mut auth_session_signal = state.auth_session;
        let mut auth_error_signal = state.auth_error;
        let mut auth_message_signal = auth_message;
        let mut auth_password_signal = auth_password;
        let mut auth_busy_signal = auth_busy;
        let mut db_reconnect_signal = state.db_reconnect_version;
        spawn(async move {
            match service.sign_in(&email, &password).await {
                Ok(session) => {
                    auth_session_signal.set(Some(session));
                    auth_error_signal.set(None);
                    auth_password_signal.set(String::new());
                    auth_message_signal.set(Some("Signed in.".to_string()));
                    db_reconnect_signal.set(db_reconnect_signal().saturating_add(1));
                }
                Err(error) => {
                    tracing::error!("Sign-in failed: {}", error);
                    let message = format_auth_error_message(&error.to_string());
                    auth_error_signal.set(Some(message.clone()));
                    auth_message_signal.set(Some(message));
                }
            }
            auth_busy_signal.set(false);
        });
    };

    let sign_up = move |_: MouseEvent| {
        let Some(service) = state.auth_service.read().clone() else {
            auth_message.set(Some(
                "Authentication is not available in this build.".to_string(),
            ));
            return;
        };

        if auth_verifying() {
            auth_message.set(Some(
                "Auth configuration check is still running.".to_string(),
            ));
            return;
        }

        if let Some(reason) = sign_up_block_reason(auth_config_status()) {
            auth_message.set(Some(reason));
            return;
        }

        let email = auth_email().trim().to_string();
        let password = auth_password();
        if email.is_empty() || password.trim().is_empty() {
            auth_message.set(Some("Email and password are required.".to_string()));
            return;
        }

        auth_busy.set(true);
        auth_message.set(None);

        let mut auth_session_signal = state.auth_session;
        let mut auth_error_signal = state.auth_error;
        let mut auth_message_signal = auth_message;
        let mut auth_busy_signal = auth_busy;
        let mut db_reconnect_signal = state.db_reconnect_version;
        spawn(async move {
            match service.sign_up(&email, &password).await {
                Ok(SignUpOutcome::SignedIn(session)) => {
                    auth_session_signal.set(Some(session));
                    auth_error_signal.set(None);
                    auth_message_signal.set(Some("Account created and signed in.".to_string()));
                    db_reconnect_signal.set(db_reconnect_signal().saturating_add(1));
                }
                Ok(SignUpOutcome::ConfirmationRequired) => {
                    auth_error_signal.set(None);
                    auth_message_signal.set(Some(
                        "Sign-up succeeded. Confirm your email, then sign in.".to_string(),
                    ));
                }
                Err(error) => {
                    tracing::error!("Sign-up failed: {}", error);
                    let message = format_auth_error_message(&error.to_string());
                    auth_error_signal.set(Some(message.clone()));
                    auth_message_signal.set(Some(message));
                }
            }
            auth_busy_signal.set(false);
        });
    };

    let sign_out = move |_: MouseEvent| {
        let Some(service) = state.auth_service.read().clone() else {
            auth_message.set(Some(
                "Authentication is not available in this build.".to_string(),
            ));
            return;
        };
        let Some(session) = (state.auth_session)() else {
            auth_message.set(Some("No active session.".to_string()));
            return;
        };

        auth_busy.set(true);
        auth_message.set(None);

        let mut auth_session_signal = state.auth_session;
        let mut auth_error_signal = state.auth_error;
        let mut auth_message_signal = auth_message;
        let mut auth_busy_signal = auth_busy;
        let mut db_reconnect_signal = state.db_reconnect_version;
        spawn(async move {
            match service.sign_out(&session.access_token).await {
                Ok(()) => {
                    auth_session_signal.set(None);
                    auth_error_signal.set(None);
                    auth_message_signal.set(Some("Signed out.".to_string()));
                    db_reconnect_signal.set(db_reconnect_signal().saturating_add(1));
                }
                Err(error) => {
                    tracing::error!("Sign-out failed: {}", error);
                    let message = format_auth_error_message(&error.to_string());
                    auth_error_signal.set(Some(message.clone()));
                    auth_message_signal.set(Some(message));
                }
            }
            auth_busy_signal.set(false);
        });
    };

    let verify_config = move |_: MouseEvent| {
        let Some(service) = state.auth_service.read().clone() else {
            auth_message.set(Some(
                "Authentication is not available in this build.".to_string(),
            ));
            return;
        };

        auth_verifying.set(true);
        auth_message.set(None);

        let mut auth_error_signal = state.auth_error;
        let mut auth_message_signal = auth_message;
        let mut auth_verifying_signal = auth_verifying;
        let mut auth_config_status_signal = auth_config_status;
        spawn(async move {
            match service.verify_configuration().await {
                Ok(status) => {
                    auth_error_signal.set(None);
                    auth_config_status_signal.set(Some(status));
                    auth_message_signal.set(Some(format_auth_config_status(status)));
                }
                Err(error) => {
                    tracing::error!("Auth config verify failed: {}", error);
                    let message = format_auth_error_message(&error.to_string());
                    auth_error_signal.set(Some(message.clone()));
                    auth_message_signal.set(Some(message));
                    auth_config_status_signal.set(None);
                }
            }
            auth_verifying_signal.set(false);
        });
    };

    let export_json = move |_: MouseEvent| {
        if export_busy() {
            return;
        }

        export_busy.set(true);
        export_message.set(None);

        let db = state.db_service.read().clone();
        let mut export_busy_signal = export_busy;
        let mut export_message_signal = export_message;
        spawn(async move {
            let Some(db) = db else {
                export_message_signal.set(Some("Database service is not available.".to_string()));
                export_busy_signal.set(false);
                return;
            };

            let default_name = suggested_export_file_name(
                NotesExportFormat::Json,
                chrono::Utc::now().timestamp_millis(),
            );
            let Some(file) = AsyncFileDialog::new()
                .set_file_name(&default_name)
                .save_file()
                .await
            else {
                export_busy_signal.set(false);
                return;
            };

            match export_notes_to_path(db.as_ref(), NotesExportFormat::Json, file.path()).await {
                Ok(count) => {
                    export_message_signal.set(Some(format!(
                        "Exported {count} notes to {}",
                        file.path().display()
                    )));
                }
                Err(error) => {
                    export_message_signal.set(Some(format!("Export failed: {error}")));
                }
            }
            export_busy_signal.set(false);
        });
    };

    let export_markdown = move |_: MouseEvent| {
        if export_busy() {
            return;
        }

        export_busy.set(true);
        export_message.set(None);

        let db = state.db_service.read().clone();
        let mut export_busy_signal = export_busy;
        let mut export_message_signal = export_message;
        spawn(async move {
            let Some(db) = db else {
                export_message_signal.set(Some("Database service is not available.".to_string()));
                export_busy_signal.set(false);
                return;
            };

            let default_name = suggested_export_file_name(
                NotesExportFormat::Markdown,
                chrono::Utc::now().timestamp_millis(),
            );
            let Some(file) = AsyncFileDialog::new()
                .set_file_name(&default_name)
                .save_file()
                .await
            else {
                export_busy_signal.set(false);
                return;
            };

            match export_notes_to_path(db.as_ref(), NotesExportFormat::Markdown, file.path()).await
            {
                Ok(count) => {
                    export_message_signal.set(Some(format!(
                        "Exported {count} notes to {}",
                        file.path().display()
                    )));
                }
                Err(error) => {
                    export_message_signal.set(Some(format!("Export failed: {error}")));
                }
            }
            export_busy_signal.set(false);
        });
    };

    let refresh_sync_conflicts = move |_: MouseEvent| {
        sync_conflicts_refresh_version.set(sync_conflicts_refresh_version().saturating_add(1));
    };

    let save_openai_api_key = move |_: MouseEvent| {
        let api_key = openai_api_key_input().trim().to_string();
        if api_key.is_empty() {
            openai_api_key_message.set(Some("Enter an OpenAI API key.".to_string()));
            return;
        }

        match TranscriptionService::store_api_key(&api_key) {
            Ok(()) => {
                openai_api_key_input.set(String::new());
                openai_api_key_configured.set(true);
                openai_api_key_message.set(Some(
                    "OpenAI API key saved to secure OS storage.".to_string(),
                ));
            }
            Err(error) => {
                openai_api_key_message.set(Some(format!("Failed to save API key: {error}")));
            }
        }

        match TranscriptionService::new() {
            Ok(service) => transcription_service_signal.set(Some(Arc::new(service))),
            Err(error) => {
                tracing::warn!("Voice transcription service unavailable: {}", error);
                transcription_service_signal.set(None);
            }
        }
    };

    let clear_openai_api_key = move |_: MouseEvent| {
        match TranscriptionService::clear_api_key() {
            Ok(()) => {
                openai_api_key_input.set(String::new());
                openai_api_key_configured.set(false);
                openai_api_key_message.set(Some("OpenAI API key cleared.".to_string()));
            }
            Err(error) => {
                openai_api_key_message.set(Some(format!("Failed to clear API key: {error}")));
            }
        }

        match TranscriptionService::new() {
            Ok(service) => transcription_service_signal.set(Some(Arc::new(service))),
            Err(error) => {
                tracing::warn!("Voice transcription service unavailable: {}", error);
                transcription_service_signal.set(None);
            }
        }
    };

    let auth_working = auth_busy() || auth_verifying();
    let sign_up_blocked_reason = sign_up_block_reason(auth_config_status());
    let sign_up_blocked = sign_up_blocked_reason.is_some();

    rsx! {
        DialogRoot {
            open: true,
            on_open_change: move |open: bool| {
                if !open {
                    settings_open.set(false);
                }
            },

            DialogContent {
                style: "width: 400px; max-width: 90vw; text-align: left;",

                // Header with close button
                div {
                    style: "
                        display: flex;
                        justify-content: space-between;
                        align-items: center;
                        margin-bottom: 8px;
                    ",
                    DialogTitle { "Settings" }
                    Button {
                        variant: ButtonVariant::Ghost,
                        onclick: close_settings,
                        style: "padding: 4px 8px; font-size: 18px;",
                        "×"
                    }
                }

                // Theme setting
                SettingRow {
                    label: "Theme",
                    description: "Choose your preferred color scheme",

                    Select::<String> {
                        default_value: current_theme_value.to_string(),
                        on_value_change: {
                            let mut save = save_settings;
                            move |value: Option<String>| {
                                if let Some(value) = value {
                                    let new_theme = match value.as_str() {
                                        "light" => ThemeMode::Light,
                                        "dark" => ThemeMode::Dark,
                                        _ => ThemeMode::System,
                                    };
                                    let mut new_settings = settings();
                                    new_settings.theme = new_theme;
                                    save(new_settings);
                                }
                            }
                        },

                        SelectTrigger {
                            style: "width: 150px;",
                            SelectValue {}
                        }

                        SelectList {
                            SelectOption::<String> {
                                index: 0usize,
                                value: "system".to_string(),
                                text_value: "System",
                                "System"
                                SelectItemIndicator {}
                            }
                            SelectOption::<String> {
                                index: 1usize,
                                value: "light".to_string(),
                                text_value: "Light",
                                "Light"
                                SelectItemIndicator {}
                            }
                            SelectOption::<String> {
                                index: 2usize,
                                value: "dark".to_string(),
                                text_value: "Dark",
                                "Dark"
                                SelectItemIndicator {}
                            }
                        }
                    }
                }

                // Font family setting
                SettingRow {
                    label: "Font Family",
                    description: "Font used for note content",

                    Select::<String> {
                        default_value: current_settings.font_family.clone(),
                        on_value_change: {
                            let mut save = save_settings;
                            move |value: Option<String>| {
                                if let Some(value) = value {
                                    let mut new_settings = settings();
                                    new_settings.font_family = value;
                                    save(new_settings);
                                }
                            }
                        },

                        SelectTrigger {
                            style: "width: 170px;",
                            SelectValue {}
                        }

                        SelectList {
                            for (i, (value, label)) in FONT_FAMILIES.iter().enumerate() {
                                SelectOption::<String> {
                                    key: "{value}",
                                    index: i,
                                    value: (*value).to_string(),
                                    text_value: *label,
                                    "{label}"
                                    SelectItemIndicator {}
                                }
                            }
                        }
                    }
                }

                // Font size setting
                SettingRow {
                    label: "Font Size",
                    description: "Size of text in notes (10-24px)",

                    div {
                        style: "display: flex; align-items: center; gap: 8px;",
                        Slider {
                            min: 10.0,
                            max: 24.0,
                            step: 1.0,
                            default_value: SliderValue::Single(f64::from(current_settings.font_size)),
                            on_value_change: {
                                let mut save = save_settings;
                                move |slider_value: SliderValue| {
                                    let SliderValue::Single(size) = slider_value;
                                    let mut new_settings = settings();
                                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                                    {
                                        new_settings.font_size = (size as u32).clamp(10, 24);
                                    }
                                    save(new_settings);
                                }
                            },
                            style: "width: 100px;",

                            SliderTrack {
                                SliderRange {}
                            }
                            SliderThumb {}
                        }
                        span {
                            class: "slider-value",
                            "{current_settings.font_size}px"
                        }
                    }
                }

                // Hotkey display (read-only for now)
                SettingRow {
                    label: "Capture Hotkey",
                    description: "Global shortcut for quick capture",

                    div {
                        class: "hotkey-display",
                        style: "
                            background: {colors.bg_tertiary};
                            border: 1px solid {colors.border};
                        ",
                        "Ctrl + Alt + N"
                    }
                }

                SettingRow {
                    label: "Voice Transcription",
                    description: "{transcription_status_text}",

                    div {
                        class: "auth-actions",
                        Button {
                            variant: if current_settings.voice_memo_transcription_enabled {
                                ButtonVariant::Secondary
                            } else {
                                ButtonVariant::Ghost
                            },
                            disabled: transcription_toggle_disabled,
                            onclick: {
                                let mut save = save_settings;
                                move |_| {
                                    let mut new_settings = settings();
                                    new_settings.voice_memo_transcription_enabled =
                                        !new_settings.voice_memo_transcription_enabled;
                                    save(new_settings);
                                }
                            },
                            if current_settings.voice_memo_transcription_enabled {
                                "Enabled"
                            } else {
                                "Disabled"
                            }
                        }
                    }
                }

                SettingRow {
                    label: "API Keys",
                    description: "Store user-provided API keys in the OS keychain.",

                    div {
                        class: "auth-panel",

                        Input {
                            class: "auth-input",
                            r#type: "password",
                            placeholder: "OpenAI API key",
                            value: "{openai_api_key_input}",
                            oninput: move |event: FormEvent| {
                                openai_api_key_input.set(event.value());
                            },
                        }

                        div {
                            class: "auth-actions",
                            Button {
                                variant: ButtonVariant::Secondary,
                                onclick: save_openai_api_key,
                                "Save Key"
                            }
                            Button {
                                variant: ButtonVariant::Ghost,
                                onclick: clear_openai_api_key,
                                "Clear Key"
                            }
                        }

                        div {
                            class: "auth-hint",
                            if openai_api_key_configured() {
                                "OpenAI API key is stored securely."
                            } else {
                                "No secure OpenAI API key is currently stored."
                            }
                        }

                        if let Some(message) = openai_api_key_message() {
                            div {
                                class: "auth-message",
                                "{message}"
                            }
                        }
                    }
                }

                SettingRow {
                    label: "Export",
                    description: "Export all notes as JSON or Markdown",

                    div {
                        class: "auth-panel",
                        div {
                            class: "auth-actions",
                            Button {
                                variant: ButtonVariant::Secondary,
                                disabled: export_busy(),
                                onclick: export_json,
                                "Export JSON"
                            }
                            Button {
                                variant: ButtonVariant::Secondary,
                                disabled: export_busy(),
                                onclick: export_markdown,
                                "Export Markdown"
                            }
                        }

                        if export_busy() {
                            div {
                                class: "auth-message",
                                "Exporting..."
                            }
                        }

                        if let Some(message) = export_message() {
                            div {
                                class: "auth-message",
                                "{message}"
                            }
                        }
                    }
                }

                SettingRow {
                    label: "Offline Queue",
                    description: "Pending local changes waiting for sync",

                    div {
                        class: "auth-panel",
                        div {
                            class: "auth-hint",
                            "Pending changes: {pending_sync_count}"
                        }
                        if pending_sync_count > 0 {
                            div {
                                class: "auth-hint",
                                "Pending note IDs: {pending_sync_preview}"
                            }
                        }
                    }
                }

                SettingRow {
                    label: "Sync Conflicts",
                    description: "Recent LWW conflict resolutions",

                    div {
                        class: "auth-panel",
                        div {
                            class: "auth-actions",
                            Button {
                                variant: ButtonVariant::Secondary,
                                disabled: sync_conflicts_loading(),
                                onclick: refresh_sync_conflicts,
                                "Refresh"
                            }
                        }

                        if sync_conflicts_loading() {
                            div {
                                class: "auth-message",
                                "Loading recent conflicts..."
                            }
                        } else if let Some(error) = sync_conflicts_error() {
                            div {
                                class: "auth-error",
                                "{error}"
                            }
                        } else if sync_conflicts().is_empty() {
                            div {
                                class: "auth-hint",
                                "No sync conflicts recorded yet."
                            }
                        } else {
                            div {
                                style: "display: flex; flex-direction: column; gap: 8px;",
                                for conflict in sync_conflicts() {
                                    div {
                                        key: "{conflict.id}",
                                        style: "padding: 8px; border: 1px solid #37415133; border-radius: 8px;",
                                        div {
                                            style: "font-size: 12px; font-weight: 600;",
                                            "Note {conflict.note_id}"
                                        }
                                        div {
                                            style: "font-size: 11px; opacity: 0.9;",
                                            "Resolved: {format_sync_conflict_timestamp(conflict.resolved_at)}"
                                        }
                                        div {
                                            style: "font-size: 11px; opacity: 0.9;",
                                            "Local ts: {conflict.local_updated_at}, incoming ts: {conflict.incoming_updated_at}, strategy: {conflict.strategy}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Account authentication
                SettingRow {
                    label: "Account",
                    description: "Sign in with Supabase for cloud sync",

                    div {
                        class: "auth-panel",

                        if let Some(identity) = &signed_in_identity {
                            div {
                                class: "auth-status",
                                "Signed in as {identity}"
                            }
                            Button {
                                variant: ButtonVariant::Secondary,
                                disabled: auth_working,
                                onclick: sign_out,
                                "Sign Out"
                            }
                        } else if auth_service.is_some() {
                            Input {
                                class: "auth-input",
                                r#type: "email",
                                placeholder: "Email",
                                value: "{auth_email}",
                                disabled: auth_working,
                                oninput: move |event: FormEvent| {
                                    auth_email.set(event.value());
                                },
                            }
                            Input {
                                class: "auth-input",
                                r#type: "password",
                                placeholder: "Password",
                                value: "{auth_password}",
                                disabled: auth_working,
                                oninput: move |event: FormEvent| {
                                    auth_password.set(event.value());
                                },
                            }
                            div {
                                class: "auth-actions",
                                Button {
                                    variant: ButtonVariant::Primary,
                                    disabled: auth_working,
                                    onclick: sign_in,
                                    "Sign In"
                                }
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    disabled: auth_working || sign_up_blocked,
                                    onclick: sign_up,
                                    "Sign Up"
                                }
                            }
                        } else {
                            div {
                                class: "auth-hint",
                                "Authentication is unavailable in this build."
                            }
                        }

                        if auth_service.is_some() {
                            Button {
                                variant: ButtonVariant::Ghost,
                                disabled: auth_working,
                                onclick: verify_config,
                                "Verify Config"
                            }
                        }

                        if auth_working {
                            div {
                                class: "auth-message",
                                "Working..."
                            }
                        }

                        if let Some(reason) = &sign_up_blocked_reason {
                            div {
                                class: "auth-hint",
                                "{reason}"
                            }
                        }

                        if let Some(status) = auth_config_status() {
                            div {
                                class: "auth-hint",
                                "{format_auth_config_status(status)}"
                            }
                        }

                        if let Some(message) = auth_message() {
                            div {
                                class: "auth-message",
                                "{message}"
                            }
                        }

                        if let Some(error_message) = init_auth_error {
                            div {
                                class: "auth-error",
                                "{error_message}"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Individual setting row
#[component]
fn SettingRow(
    #[props(into)] label: String,
    #[props(into)] description: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "settings-row",

            div {
                class: "settings-row-info",
                div {
                    class: "settings-row-label",
                    "{label}"
                }
                div {
                    class: "settings-row-description",
                    "{description}"
                }
            }
            div {
                class: "settings-row-control",
                {children}
            }
        }
    }
}

fn transcription_status_text(status: Option<&TranscriptionConfigStatus>, enabled: bool) -> String {
    let toggle = if enabled { "enabled" } else { "disabled" };

    match status {
        Some(status) if status.enabled => {
            let model = status.model.as_deref().unwrap_or("default");
            format!(
                "Optional transcription is {toggle}. Provider: {} ({model}).",
                status.provider
            )
        }
        Some(_) => {
            format!("Optional transcription is {toggle}. Add an OpenAI API key in API Keys.")
        }
        None => {
            format!("Optional transcription is {toggle}. Service failed to initialize.")
        }
    }
}

fn sign_up_block_reason(status: Option<AuthConfigStatus>) -> Option<String> {
    let status = status?;

    if !status.email_enabled {
        return Some(
            "Sign-up unavailable: email provider is disabled in Supabase Auth.".to_string(),
        );
    }
    if !status.signup_enabled {
        return Some(
            "Sign-up unavailable: disable_signup is enabled in Supabase Auth.".to_string(),
        );
    }
    if !status.mailer_autoconfirm && !status.smtp_configured {
        return Some(
            "Sign-up is blocked until SMTP is configured or mailer autoconfirm is enabled."
                .to_string(),
        );
    }

    None
}

fn format_auth_error_message(raw: &str) -> String {
    let normalized = raw.to_lowercase();
    if normalized.contains("over_email_send_rate_limit")
        || normalized.contains("email rate limit exceeded")
        || normalized.contains("(429)")
    {
        return "Sign-up email rate limit reached. For dev, enable mailer autoconfirm in Supabase Auth. For production, configure custom SMTP.".to_string();
    }
    if normalized.contains("email address") && normalized.contains("invalid")
        || normalized.contains("invalid email")
    {
        return "Email address was rejected by Supabase. Use a valid address format and avoid disposable/test-only domains.".to_string();
    }
    if normalized.contains("http request failed")
        || normalized.contains("connection")
        || normalized.contains("timed out")
    {
        return "Network error while contacting Supabase Auth. Check your internet connection."
            .to_string();
    }

    raw.to_string()
}

fn format_auth_config_status(status: AuthConfigStatus) -> String {
    if !status.email_enabled {
        return "Auth config check: email provider is disabled in Supabase Auth.".to_string();
    }
    if !status.signup_enabled {
        return "Auth config check: sign-up is disabled in Supabase Auth.".to_string();
    }
    if !status.mailer_autoconfirm && !status.smtp_configured {
        return status.rate_limit_email_sent.map_or_else(
            || "Auth config check: signup works, but email confirmation requires SMTP. Configure custom SMTP or enable autoconfirm for dev.".to_string(),
            |limit| {
                format!(
                    "Auth config check: signup works, but email confirmation requires SMTP. Current email send rate limit is {limit}/hour."
                )
            },
        );
    }

    "Auth config check passed.".to_string()
}

fn format_sync_conflict_timestamp(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms).map_or_else(
        || timestamp_ms.to_string(),
        |date_time| date_time.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
}

fn format_pending_sync_preview(note_ids: &[NoteId]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_error_message_maps_rate_limit() {
        let message = format_auth_error_message("Auth API error: email rate limit exceeded (429)");
        assert!(message.contains("rate limit"));
        assert!(message.contains("SMTP"));
    }

    #[test]
    fn auth_config_message_highlights_missing_smtp() {
        let status = AuthConfigStatus {
            email_enabled: true,
            signup_enabled: true,
            mailer_autoconfirm: false,
            smtp_configured: false,
            rate_limit_email_sent: Some(2),
        };
        let message = format_auth_config_status(status);
        assert!(message.contains("SMTP"));
        assert!(message.contains("2/hour"));
    }

    #[test]
    fn sign_up_block_reason_when_signup_disabled() {
        let status = AuthConfigStatus {
            email_enabled: true,
            signup_enabled: false,
            mailer_autoconfirm: true,
            smtp_configured: false,
            rate_limit_email_sent: None,
        };

        let reason = sign_up_block_reason(Some(status)).unwrap();
        assert!(reason.contains("Sign-up unavailable"));
        assert!(reason.contains("disable_signup"));
    }

    #[test]
    fn sign_up_block_reason_when_missing_smtp_and_autoconfirm() {
        let status = AuthConfigStatus {
            email_enabled: true,
            signup_enabled: true,
            mailer_autoconfirm: false,
            smtp_configured: false,
            rate_limit_email_sent: Some(2),
        };

        let reason = sign_up_block_reason(Some(status)).unwrap();
        assert!(reason.contains("blocked"));
        assert!(reason.contains("SMTP"));
    }

    #[test]
    fn sign_up_block_reason_allows_safe_signup_config() {
        let status = AuthConfigStatus {
            email_enabled: true,
            signup_enabled: true,
            mailer_autoconfirm: true,
            smtp_configured: false,
            rate_limit_email_sent: None,
        };

        assert!(sign_up_block_reason(Some(status)).is_none());
    }

    #[test]
    fn format_sync_conflict_timestamp_uses_utc_display() {
        let formatted = format_sync_conflict_timestamp(0);
        assert_eq!(formatted, "1970-01-01 00:00:00 UTC");
    }

    #[test]
    fn format_pending_sync_preview_shows_overflow_suffix() {
        let ids = vec![
            "11111111-1111-7111-8111-111111111111".parse().unwrap(),
            "11111111-1111-7111-8111-222222222222".parse().unwrap(),
            "11111111-1111-7111-8111-333333333333".parse().unwrap(),
            "11111111-1111-7111-8111-444444444444".parse().unwrap(),
            "11111111-1111-7111-8111-555555555555".parse().unwrap(),
            "11111111-1111-7111-8111-666666666666".parse().unwrap(),
        ];
        let preview = format_pending_sync_preview(&ids);
        assert!(preview.contains("+1"));
    }
}
