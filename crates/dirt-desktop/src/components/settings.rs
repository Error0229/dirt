//! Settings panel component

use dioxus::prelude::*;
use dioxus_primitives::slider::SliderValue;

use dirt_core::models::{Settings, ThemeMode};

use super::button::{Button, ButtonVariant};
use super::dialog::{DialogContent, DialogRoot, DialogTitle};
use super::input::Input;
use super::select::{
    Select, SelectItemIndicator, SelectList, SelectOption, SelectTrigger, SelectValue,
};
use super::slider::{Slider, SliderRange, SliderThumb, SliderTrack};
use crate::services::SignUpOutcome;
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
    let active_session = (state.auth_session)();
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

    let sign_in = move |_: MouseEvent| {
        let Some(service) = state.auth_service.read().clone() else {
            auth_message.set(Some(
                "Supabase auth is not configured. Set SUPABASE_URL and SUPABASE_ANON_KEY."
                    .to_string(),
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
        spawn(async move {
            match service.sign_in(&email, &password).await {
                Ok(session) => {
                    auth_session_signal.set(Some(session));
                    auth_error_signal.set(None);
                    auth_password_signal.set(String::new());
                    auth_message_signal.set(Some("Signed in.".to_string()));
                }
                Err(error) => {
                    tracing::error!("Sign-in failed: {}", error);
                    auth_error_signal.set(Some(error.to_string()));
                    auth_message_signal.set(Some(error.to_string()));
                }
            }
            auth_busy_signal.set(false);
        });
    };

    let sign_up = move |_: MouseEvent| {
        let Some(service) = state.auth_service.read().clone() else {
            auth_message.set(Some(
                "Supabase auth is not configured. Set SUPABASE_URL and SUPABASE_ANON_KEY."
                    .to_string(),
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
        let mut auth_busy_signal = auth_busy;
        spawn(async move {
            match service.sign_up(&email, &password).await {
                Ok(SignUpOutcome::SignedIn(session)) => {
                    auth_session_signal.set(Some(session));
                    auth_error_signal.set(None);
                    auth_message_signal.set(Some("Account created and signed in.".to_string()));
                }
                Ok(SignUpOutcome::ConfirmationRequired) => {
                    auth_error_signal.set(None);
                    auth_message_signal.set(Some(
                        "Sign-up succeeded. Confirm your email, then sign in.".to_string(),
                    ));
                }
                Err(error) => {
                    tracing::error!("Sign-up failed: {}", error);
                    auth_error_signal.set(Some(error.to_string()));
                    auth_message_signal.set(Some(error.to_string()));
                }
            }
            auth_busy_signal.set(false);
        });
    };

    let sign_out = move |_: MouseEvent| {
        let Some(service) = state.auth_service.read().clone() else {
            auth_message.set(Some(
                "Supabase auth is not configured. Set SUPABASE_URL and SUPABASE_ANON_KEY."
                    .to_string(),
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
        spawn(async move {
            match service.sign_out(&session.access_token).await {
                Ok(()) => {
                    auth_session_signal.set(None);
                    auth_error_signal.set(None);
                    auth_message_signal.set(Some("Signed out.".to_string()));
                }
                Err(error) => {
                    tracing::error!("Sign-out failed: {}", error);
                    auth_error_signal.set(Some(error.to_string()));
                    auth_message_signal.set(Some(error.to_string()));
                }
            }
            auth_busy_signal.set(false);
        });
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
                                disabled: auth_busy(),
                                onclick: sign_out,
                                "Sign Out"
                            }
                        } else if auth_service.is_some() {
                            Input {
                                class: "auth-input",
                                r#type: "email",
                                placeholder: "Email",
                                value: "{auth_email}",
                                disabled: auth_busy(),
                                oninput: move |event: FormEvent| {
                                    auth_email.set(event.value());
                                },
                            }
                            Input {
                                class: "auth-input",
                                r#type: "password",
                                placeholder: "Password",
                                value: "{auth_password}",
                                disabled: auth_busy(),
                                oninput: move |event: FormEvent| {
                                    auth_password.set(event.value());
                                },
                            }
                            div {
                                class: "auth-actions",
                                Button {
                                    variant: ButtonVariant::Primary,
                                    disabled: auth_busy(),
                                    onclick: sign_in,
                                    "Sign In"
                                }
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    disabled: auth_busy(),
                                    onclick: sign_up,
                                    "Sign Up"
                                }
                            }
                        } else {
                            div {
                                class: "auth-hint",
                                "Set SUPABASE_URL and SUPABASE_ANON_KEY in .env to enable authentication."
                            }
                        }

                        if auth_busy() {
                            div {
                                class: "auth-message",
                                "Working..."
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
