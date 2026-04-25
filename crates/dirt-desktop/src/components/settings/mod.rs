//! Settings panel component.
//!
//! Auth tab was removed in the Supabase teardown; sign-in/sign-out lived
//! here when the desktop app held a Supabase session. The Sync tab still
//! shows the high-level status signals — they currently stay at
//! `Offline` until the new ApiClient-driven worker lands.

use std::sync::Arc;

use dioxus::prelude::*;
use rfd::AsyncFileDialog;

use dirt_core::models::{NoteId, Settings, ThemeMode};

use super::button::{Button, ButtonVariant};
use super::dialog::{DialogContent, DialogRoot, DialogTitle};
use crate::services::{
    NotesExportFormat, TranscriptionConfigStatus, TranscriptionService, export_notes_to_path,
    suggested_export_file_name,
};
use crate::state::AppState;
use crate::theme::resolve_theme;
use media_settings::MediaSettingsTab;
use sync_settings::SyncSettingsTab;
use theme_settings::ThemeSettingsTab;

mod media_settings;
mod row;
mod sync_settings;
mod theme_settings;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Appearance,
    Media,
    Sync,
}

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
    let sync_status = (state.sync_status)();
    let sync_issue = (state.sync_issue)();
    let pending_sync_count = (state.pending_sync_count)();
    let pending_sync_note_ids = (state.pending_sync_note_ids)();
    let pending_sync_preview = format_pending_sync_preview(&pending_sync_note_ids);
    let mut export_busy = use_signal(|| false);
    let mut export_message = use_signal(|| None::<String>);

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

    let mut active_tab = use_signal(|| SettingsTab::Appearance);

    let on_theme_change = {
        let mut save = save_settings;
        move |value: String| {
            let new_theme = match value.as_str() {
                "light" => ThemeMode::Light,
                "dark" => ThemeMode::Dark,
                _ => ThemeMode::System,
            };
            let mut new_settings = settings();
            new_settings.theme = new_theme;
            save(new_settings);
        }
    };

    let on_font_family_change = {
        let mut save = save_settings;
        move |value: String| {
            let mut new_settings = settings();
            new_settings.font_family = value;
            save(new_settings);
        }
    };

    let on_font_size_change = {
        let mut save = save_settings;
        move |font_size: u32| {
            let mut new_settings = settings();
            new_settings.font_size = font_size;
            save(new_settings);
        }
    };

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

                div {
                    style: "display: flex; gap: 8px; margin-bottom: 12px;",
                    Button {
                        variant: if active_tab() == SettingsTab::Appearance {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        },
                        onclick: move |_| active_tab.set(SettingsTab::Appearance),
                        "Appearance"
                    }
                    Button {
                        variant: if active_tab() == SettingsTab::Media {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        },
                        onclick: move |_| active_tab.set(SettingsTab::Media),
                        "Media"
                    }
                    Button {
                        variant: if active_tab() == SettingsTab::Sync {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        },
                        onclick: move |_| active_tab.set(SettingsTab::Sync),
                        "Sync"
                    }
                }

                match active_tab() {
                    SettingsTab::Appearance => rsx! {
                        ThemeSettingsTab {
                            hotkey_bg: colors.bg_tertiary,
                            hotkey_border: colors.border,
                            current_settings: current_settings,
                            current_theme_value: current_theme_value.to_string(),
                            on_theme_change: on_theme_change,
                            on_font_family_change: on_font_family_change,
                            on_font_size_change: on_font_size_change,
                        }
                    },
                    SettingsTab::Media => rsx! {
                        MediaSettingsTab {
                            current_settings: current_settings,
                            transcription_status_text: transcription_status_text,
                            transcription_toggle_disabled: transcription_toggle_disabled,
                            on_toggle_transcription: {
                                let mut save = save_settings;
                                move |_| {
                                    let mut new_settings = settings();
                                    new_settings.voice_memo_transcription_enabled =
                                        !new_settings.voice_memo_transcription_enabled;
                                    save(new_settings);
                                }
                            },
                            openai_api_key_input: openai_api_key_input(),
                            on_openai_api_key_input: move |value: String| {
                                openai_api_key_input.set(value);
                            },
                            on_save_openai_api_key: save_openai_api_key,
                            on_clear_openai_api_key: clear_openai_api_key,
                            openai_api_key_configured: openai_api_key_configured(),
                            openai_api_key_message: openai_api_key_message(),
                            export_busy: export_busy(),
                            on_export_json: export_json,
                            on_export_markdown: export_markdown,
                            export_message: export_message(),
                        }
                    },
                    SettingsTab::Sync => rsx! {
                        SyncSettingsTab {
                            sync_status: sync_status,
                            sync_issue: sync_issue,
                            pending_sync_count: pending_sync_count,
                            pending_sync_preview: pending_sync_preview,
                        }
                    },
                }
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
