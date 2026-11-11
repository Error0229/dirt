//! Settings panel component.
//!
//! The Account tab drives the magic-code sign-in/sign-out flow against
//! `dirt-api`'s `/v1/auth/*` endpoints. The Sync tab shows the runtime
//! health of the auto-sync worker; it surfaces `Offline` until the
//! user signs in on the Account tab.

use dioxus::prelude::*;
use rfd::AsyncFileDialog;

use dirt_core::models::{NoteId, Settings, ThemeMode};

use super::button::{Button, ButtonVariant};
use super::dialog::{DialogContent, DialogRoot, DialogTitle};
use crate::services::{export_notes_to_path, suggested_export_file_name, NotesExportFormat};
use crate::state::AppState;
use crate::theme::resolve_theme;
use account_settings::AccountSettingsTab;
use data_settings::DataSettingsTab;
use sync_settings::SyncSettingsTab;
use theme_settings::ThemeSettingsTab;

mod account_settings;
mod data_settings;
mod row;
mod sync_settings;
mod theme_settings;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Appearance,
    Data,
    Sync,
    Account,
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
    let sync_status = (state.sync_status)();
    let sync_issue = (state.sync_issue)();
    let pending_sync_count = (state.pending_sync_count)();
    let pending_sync_note_ids = (state.pending_sync_note_ids)();
    let pending_sync_preview = format_pending_sync_preview(&pending_sync_note_ids);
    let mut export_busy = use_signal(|| false);
    let mut export_message = use_signal(|| None::<String>);

    let mut export_notes = move |format: NotesExportFormat| {
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

            let default_name =
                suggested_export_file_name(format, chrono::Utc::now().timestamp_millis());
            let Some(file) = AsyncFileDialog::new()
                .set_file_name(&default_name)
                .save_file()
                .await
            else {
                export_busy_signal.set(false);
                return;
            };

            match export_notes_to_path(db.as_ref(), format, file.path()).await {
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

    let export_json = move |_: MouseEvent| export_notes(NotesExportFormat::Json);
    let export_markdown = move |_: MouseEvent| export_notes(NotesExportFormat::Markdown);

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
                style: "text-align: left; width: 480px; max-width: 92vw; height: 520px; max-height: 80vh; padding: 0; gap: 0; border-radius: 12px; overflow: hidden;",

                // Fixed header: title + tabs
                div {
                    class: "settings-header",

                    // Title row
                    div {
                        style: "
                            display: flex;
                            justify-content: space-between;
                            align-items: center;
                            margin-bottom: 12px;
                        ",
                        DialogTitle {
                            "Settings"
                        }
                        Button {
                            variant: ButtonVariant::Ghost,
                            onclick: close_settings,
                            style: "width: 28px; height: 28px; padding: 0; font-size: 18px; display: flex; align-items: center; justify-content: center; border-radius: 6px;",
                            "×"
                        }
                    }

                    // Tab bar
                    div {
                        style: "
                            display: flex;
                            gap: 0;
                            border-bottom: 1px solid {colors.border};
                        ",
                        {
                            let tabs = [
                                (SettingsTab::Appearance, "Appearance"),
                                (SettingsTab::Data, "Data"),
                                (SettingsTab::Sync, "Sync"),
                                (SettingsTab::Account, "Account"),
                            ];
                            rsx! {
                                for (tab, label) in tabs {
                                    {
                                        let is_active = active_tab() == tab;
                                        let text_color = if is_active { colors.text_primary } else { colors.text_muted };
                                        let border_bottom = if is_active {
                                            format!("2px solid {}", colors.accent)
                                        } else {
                                            "2px solid transparent".to_string()
                                        };
                                        rsx! {
                                            button {
                                                style: "
                                                    background: none;
                                                    border: none;
                                                    border-bottom: {border_bottom};
                                                    color: {text_color};
                                                    font-size: 13px;
                                                    font-weight: 500;
                                                    padding: 8px 14px;
                                                    cursor: pointer;
                                                    transition: color 0.1s;
                                                ",
                                                onclick: move |_| active_tab.set(tab),
                                                "{label}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Scrollable body
                div {
                    class: "settings-body",

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
                    SettingsTab::Data => rsx! {
                        DataSettingsTab {
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
                    SettingsTab::Account => rsx! { AccountSettingsTab {} },
                }

                } // settings-body
            }
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
