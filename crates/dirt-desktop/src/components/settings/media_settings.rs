use dioxus::prelude::*;

use dirt_core::models::Settings;

use super::row::SettingRow;
use crate::components::button::{Button, ButtonVariant};
use crate::components::input::Input;

#[component]
pub(super) fn MediaSettingsTab(
    current_settings: Settings,
    transcription_status_text: String,
    transcription_toggle_disabled: bool,
    on_toggle_transcription: EventHandler<MouseEvent>,
    openai_api_key_input: String,
    on_openai_api_key_input: EventHandler<String>,
    on_save_openai_api_key: EventHandler<MouseEvent>,
    on_clear_openai_api_key: EventHandler<MouseEvent>,
    openai_api_key_configured: bool,
    openai_api_key_message: Option<String>,
    export_busy: bool,
    on_export_json: EventHandler<MouseEvent>,
    on_export_markdown: EventHandler<MouseEvent>,
    export_message: Option<String>,
) -> Element {
    rsx! {
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
                    onclick: move |event| on_toggle_transcription.call(event),
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
                        on_openai_api_key_input.call(event.value());
                    },
                }

                div {
                    class: "auth-actions",
                    Button {
                        variant: ButtonVariant::Secondary,
                        onclick: move |event| on_save_openai_api_key.call(event),
                        "Save Key"
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        onclick: move |event| on_clear_openai_api_key.call(event),
                        "Clear Key"
                    }
                }

                div {
                    class: "auth-hint",
                    if openai_api_key_configured {
                        "OpenAI API key is stored securely."
                    } else {
                        "No secure OpenAI API key is currently stored."
                    }
                }

                if let Some(message) = openai_api_key_message {
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
                        disabled: export_busy,
                        onclick: move |event| on_export_json.call(event),
                        "Export JSON"
                    }
                    Button {
                        variant: ButtonVariant::Secondary,
                        disabled: export_busy,
                        onclick: move |event| on_export_markdown.call(event),
                        "Export Markdown"
                    }
                }

                if export_busy {
                    div {
                        class: "auth-message",
                        "Exporting..."
                    }
                }

                if let Some(message) = export_message {
                    div {
                        class: "auth-message",
                        "{message}"
                    }
                }
            }
        }
    }
}
