//! Settings panel component

use dioxus::prelude::*;

use dirt_core::models::{Settings, ThemeMode};

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
    let mut save_settings = move |new_settings: Settings| {
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

    let close_settings = move |_| {
        settings_open.set(false);
    };

    // Handler for theme change
    let on_theme_change = move |evt: Event<FormData>| {
        let value = evt.value();
        let new_theme = match value.as_str() {
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            _ => ThemeMode::System,
        };
        let mut new_settings = settings();
        new_settings.theme = new_theme;
        save_settings(new_settings);
    };

    // Handler for font family change
    let on_font_change = move |evt: Event<FormData>| {
        let value = evt.value();
        let mut new_settings = settings();
        new_settings.font_family = value;
        save_settings(new_settings);
    };

    // Handler for font size change
    let on_size_change = move |evt: Event<FormData>| {
        let value = evt.value();
        if let Ok(size) = value.parse::<u32>() {
            let mut new_settings = settings();
            new_settings.font_size = size.clamp(10, 24);
            save_settings(new_settings);
        }
    };

    let current_settings = settings();
    let current_theme_value = match current_settings.theme {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
    };

    rsx! {
        div {
            class: "settings-overlay",
            style: "
                position: fixed;
                inset: 0;
                background: rgba(0, 0, 0, 0.5);
                display: flex;
                align-items: center;
                justify-content: center;
                z-index: 1000;
            ",
            onclick: close_settings,

            div {
                class: "settings-panel",
                style: "
                    background: {colors.bg_primary};
                    color: {colors.text_primary};
                    border-radius: 12px;
                    padding: 24px;
                    width: 400px;
                    max-width: 90vw;
                    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.3);
                ",
                onclick: move |evt| evt.stop_propagation(),

                // Header
                div {
                    style: "
                        display: flex;
                        justify-content: space-between;
                        align-items: center;
                        margin-bottom: 24px;
                    ",
                    h2 {
                        style: "margin: 0; font-size: 18px; font-weight: 600;",
                        "Settings"
                    }
                    button {
                        style: "
                            background: none;
                            border: none;
                            color: {colors.text_secondary};
                            cursor: pointer;
                            font-size: 20px;
                            padding: 4px 8px;
                        ",
                        onclick: close_settings,
                        "×"
                    }
                }

                // Theme setting
                SettingRow {
                    label: "Theme",
                    description: "Choose your preferred color scheme",
                    select {
                        style: "
                            background: {colors.bg_secondary};
                            color: {colors.text_primary};
                            border: 1px solid {colors.border};
                            border-radius: 6px;
                            padding: 8px 12px;
                            font-size: 14px;
                            cursor: pointer;
                            width: 150px;
                        ",
                        value: "{current_theme_value}",
                        onchange: on_theme_change,
                        option { value: "system", "System" }
                        option { value: "light", "Light" }
                        option { value: "dark", "Dark" }
                    }
                }

                // Font family setting
                SettingRow {
                    label: "Font Family",
                    description: "Font used for note content",
                    select {
                        style: "
                            background: {colors.bg_secondary};
                            color: {colors.text_primary};
                            border: 1px solid {colors.border};
                            border-radius: 6px;
                            padding: 8px 12px;
                            font-size: 14px;
                            cursor: pointer;
                            width: 150px;
                        ",
                        value: "{current_settings.font_family}",
                        onchange: on_font_change,
                        for (value, label) in FONT_FAMILIES {
                            option {
                                value: "{value}",
                                selected: current_settings.font_family == *value,
                                "{label}"
                            }
                        }
                    }
                }

                // Font size setting
                SettingRow {
                    label: "Font Size",
                    description: "Size of text in notes (10-24px)",
                    div {
                        style: "display: flex; align-items: center; gap: 12px;",
                        input {
                            r#type: "range",
                            min: "10",
                            max: "24",
                            value: "{current_settings.font_size}",
                            style: "width: 100px; cursor: pointer;",
                            oninput: on_size_change,
                        }
                        span {
                            style: "
                                font-size: 14px;
                                color: {colors.text_secondary};
                                min-width: 40px;
                            ",
                            "{current_settings.font_size}px"
                        }
                    }
                }

                // Hotkey display (read-only for now)
                SettingRow {
                    label: "Capture Hotkey",
                    description: "Global shortcut for quick capture",
                    div {
                        style: "
                            background: {colors.bg_tertiary};
                            border: 1px solid {colors.border};
                            border-radius: 6px;
                            padding: 8px 12px;
                            font-size: 13px;
                            font-family: monospace;
                            color: {colors.text_secondary};
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
    let state = use_context::<AppState>();
    let colors = (state.theme)().palette();

    rsx! {
        div {
            style: "
                display: flex;
                justify-content: space-between;
                align-items: center;
                padding: 16px 0;
                border-bottom: 1px solid {colors.border_light};
            ",
            div {
                style: "flex: 1;",
                div {
                    style: "font-size: 14px; font-weight: 500; margin-bottom: 4px;",
                    "{label}"
                }
                div {
                    style: "font-size: 12px; color: {colors.text_muted};",
                    "{description}"
                }
            }
            div {
                style: "flex-shrink: 0;",
                {children}
            }
        }
    }
}
