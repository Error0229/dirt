use dioxus::prelude::*;

use super::row::SettingRow;
use crate::components::button::{Button, ButtonVariant};

#[component]
pub(super) fn DataSettingsTab(
    export_busy: bool,
    on_export_json: EventHandler<MouseEvent>,
    on_export_markdown: EventHandler<MouseEvent>,
    export_message: Option<String>,
) -> Element {
    rsx! {
        SettingRow {
            label: "Export",
            description: "Export every note and its files into a folder, the notes as JSON or Markdown",

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
                    // One line for what was exported, and one for each
                    // thing to say about files that were not.
                    div {
                        class: "auth-message",
                        style: "white-space: pre-wrap;",
                        "{message}"
                    }
                }
            }
        }
    }
}
