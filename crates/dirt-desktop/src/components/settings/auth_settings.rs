use dioxus::prelude::*;

use super::row::SettingRow;
use crate::components::button::{Button, ButtonVariant};
use crate::components::input::Input;

#[component]
pub(super) fn AuthSettingsTab(
    auth_service_available: bool,
    signed_in_identity: Option<String>,
    auth_working: bool,
    auth_email: String,
    auth_password: String,
    sign_up_blocked: bool,
    sign_up_blocked_reason: Option<String>,
    auth_config_status_message: Option<String>,
    auth_message: Option<String>,
    init_auth_error: Option<String>,
    on_auth_email_input: EventHandler<String>,
    on_auth_password_input: EventHandler<String>,
    on_sign_in: EventHandler<MouseEvent>,
    on_sign_up: EventHandler<MouseEvent>,
    on_sign_out: EventHandler<MouseEvent>,
    on_verify_config: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        SettingRow {
            label: "Account",
            description: "Sign in with Supabase for cloud sync",

            div {
                class: "auth-panel",

                if let Some(identity) = signed_in_identity {
                    div {
                        class: "auth-status",
                        "Signed in as {identity}"
                    }
                    Button {
                        variant: ButtonVariant::Secondary,
                        disabled: auth_working,
                        onclick: move |event| on_sign_out.call(event),
                        "Sign Out"
                    }
                } else if auth_service_available {
                    Input {
                        class: "auth-input",
                        r#type: "email",
                        placeholder: "Email",
                        value: "{auth_email}",
                        disabled: auth_working,
                        oninput: move |event: FormEvent| {
                            on_auth_email_input.call(event.value());
                        },
                    }
                    Input {
                        class: "auth-input",
                        r#type: "password",
                        placeholder: "Password",
                        value: "{auth_password}",
                        disabled: auth_working,
                        oninput: move |event: FormEvent| {
                            on_auth_password_input.call(event.value());
                        },
                    }
                    div {
                        class: "auth-actions",
                        Button {
                            variant: ButtonVariant::Primary,
                            disabled: auth_working,
                            onclick: move |event| on_sign_in.call(event),
                            "Sign In"
                        }
                        Button {
                            variant: ButtonVariant::Secondary,
                            disabled: auth_working || sign_up_blocked,
                            onclick: move |event| on_sign_up.call(event),
                            "Sign Up"
                        }
                    }
                } else {
                    div {
                        class: "auth-hint",
                        "Authentication is unavailable in this build."
                    }
                }

                if auth_service_available {
                    Button {
                        variant: ButtonVariant::Ghost,
                        disabled: auth_working,
                        onclick: move |event| on_verify_config.call(event),
                        "Verify Config"
                    }
                }

                if auth_working {
                    div {
                        class: "auth-message",
                        "Working..."
                    }
                }

                if let Some(reason) = sign_up_blocked_reason {
                    div {
                        class: "auth-hint",
                        "{reason}"
                    }
                }

                if let Some(status) = auth_config_status_message {
                    div {
                        class: "auth-hint",
                        "{status}"
                    }
                }

                if let Some(message) = auth_message {
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
