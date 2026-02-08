//! Settings panel component

use dioxus::prelude::*;
use dioxus_primitives::slider::SliderValue;
use rfd::AsyncFileDialog;

use dirt_core::models::{Settings, ThemeMode};

use super::button::{Button, ButtonVariant};
use super::dialog::{DialogContent, DialogRoot, DialogTitle};
use super::input::Input;
use super::select::{
    Select, SelectItemIndicator, SelectList, SelectOption, SelectTrigger, SelectValue,
};
use super::slider::{Slider, SliderRange, SliderThumb, SliderTrack};
use crate::services::{
    export_notes_to_path, suggested_export_file_name, AuthConfigStatus, NotesExportFormat,
    SignUpOutcome,
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
    let mut auth_verifying = use_signal(|| false);
    let auth_config_status = use_signal(|| None::<AuthConfigStatus>);
    let mut auth_config_checked = use_signal(|| false);
    let auth_service_for_preflight = auth_service.clone();
    let mut export_busy = use_signal(|| false);
    let mut export_message = use_signal(|| None::<String>);

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
                "Supabase auth is not configured. Set SUPABASE_URL and SUPABASE_ANON_KEY."
                    .to_string(),
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
                "Supabase auth is not configured. Set SUPABASE_URL and SUPABASE_ANON_KEY."
                    .to_string(),
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
                                "Set SUPABASE_URL and SUPABASE_ANON_KEY in .env to enable authentication."
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
        return "Network error while contacting Supabase Auth. Check SUPABASE_URL and your internet connection.".to_string();
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
}
