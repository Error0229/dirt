//! Confirmation in place (interface design, sections 6.12 and 12).
//!
//! It is for the few things that cannot be undone. There is no dialog
//! over the app. The row itself turns pale red and asks. Deleting the
//! account also asks for a word to be typed.

use dioxus::prelude::*;

/// A row that can turn into a question about itself.
///
/// While `asking` is false it shows its children, which are the row. While
/// it is true it shows `question` with **Cancel** and the word for going
/// ahead. Focus goes to **Cancel** as the question appears, Tab stays in
/// it, Esc is **Cancel**, and focus returns to the row afterwards.
#[component]
pub fn ConfirmInPlace(
    asking: bool,
    /// "Remove Pixel 8? It stops getting notes."
    question: String,
    /// The word for going ahead: "Remove".
    confirm: String,
    #[props(default = "Cancel".to_owned())] cancel: String,
    /// A word that must be typed before going ahead is offered: "DELETE".
    #[props(default)]
    typed: Option<String>,
    on_confirm: EventHandler<()>,
    on_cancel: EventHandler<()>,
    children: Element,
) -> Element {
    let mut written = use_signal(String::new);
    // Each time it asks, the word is typed afresh.
    use_effect(use_reactive!(|asking| {
        if !asking {
            written.set(String::new());
        }
    }));
    let ready = typed
        .as_ref()
        .is_none_or(|word| written.read().trim() == word);
    rsx! {
        div { "data-dui-confirm-host": "",
            if asking {
                div {
                    class: "dui-confirm",
                    role: "alertdialog",
                    "aria-label": "{question}",
                    span { class: "dui-confirm-ask", "{question}" }
                    if let Some(word) = typed {
                        input {
                            r#type: "text",
                            class: "dui-confirm-word",
                            "aria-label": "Type {word}",
                            placeholder: "{word}",
                            autocomplete: "off",
                            spellcheck: "false",
                            value: "{written}",
                            oninput: move |event| written.set(event.value()),
                        }
                    }
                    button {
                        r#type: "button",
                        class: "dui-btn ghost sm",
                        "data-dui-cancel": "",
                        onclick: move |_| on_cancel.call(()),
                        "{cancel}"
                    }
                    button {
                        r#type: "button",
                        class: "dui-btn danger-fill sm",
                        disabled: !ready,
                        onclick: move |_| on_confirm.call(()),
                        "{confirm}"
                    }
                }
            } else {
                {children}
            }
        }
    }
}
