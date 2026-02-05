//! Settings panel component

use dioxus::prelude::*;
use dioxus_primitives::slider::SliderValue;

use dirt_core::models::{Settings, ThemeMode};

use super::button::{Button, ButtonVariant};
use super::dialog::{DialogContent, DialogRoot, DialogTitle};
use super::select::{Select, SelectItemIndicator, SelectList, SelectOption, SelectTrigger, SelectValue};
use super::slider::{Slider, SliderRange, SliderThumb, SliderTrack};
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

        // Save to database
        if let Some(ref db) = *db_service.read() {
            if let Err(e) = db.save_settings(&new_settings) {
                tracing::error!("Failed to save settings: {}", e);
            }
        }

        settings.set(new_settings);
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
            }
        }
    }
}

/// Individual setting row
#[component]
fn SettingRow(label: &'static str, description: &'static str, children: Element) -> Element {
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
