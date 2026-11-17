//! Confirmation in place (interface design, sections 6.12 and 12): for the
//! few things that cannot be undone. There is no dialog over the app. The
//! row itself turns pale red and asks.

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
    on_confirm: EventHandler<()>,
    on_cancel: EventHandler<()>,
    children: Element,
) -> Element {
    rsx! {
        div { "data-dui-confirm-host": "",
            if asking {
                div {
                    class: "dui-confirm",
                    role: "alertdialog",
                    "aria-label": "{question}",
                    span { class: "dui-confirm-ask", "{question}" }
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
