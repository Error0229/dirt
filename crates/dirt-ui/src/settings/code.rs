//! The code comparison (interface design, sections 5.5 and 6.13): seven
//! pictures with their names, in a row, the same on both devices.
//!
//! The pictures pop in one after another. When **They match** is pressed
//! the answer goes at once, naming the code that was on screen, and the
//! pictures gather to the middle while it goes; the script plays that
//! (`data-dui-gather`). The same code as digits is under the pictures, for
//! a device whose Dirt is older and shows only digits.
//!
//! Both answers can be given once: after a press the buttons wait.

use dioxus::prelude::*;
use dirt_core::enrolment::ShortCode;

use super::rows::Btn;
use crate::icons::{Icon, IconSize, IconSvg};

/// The code, and the two answers to it.
#[component]
pub fn CodeComparison(
    code: ShortCode,
    /// "Does Work laptop show these?"
    question: String,
    /// A line above the pictures, when there is something to say first:
    /// the pictures changed since the last answer.
    #[props(default)]
    note: Option<String>,
    /// An answer was given for this code: the buttons wait.
    #[props(default)]
    answered: bool,
    /// "They match", with the code that was on screen when it was pressed.
    on_match: EventHandler<ShortCode>,
    on_differ: EventHandler<()>,
) -> Element {
    let pictures = code.pictures();
    let read_out: Vec<&str> = pictures.iter().map(|picture| picture.name).collect();
    rsx! {
        div { class: "dui-pair",
            if let Some(note) = note {
                div { class: "dui-pair-note", role: "alert", "{note}" }
            }
            div { class: "dui-pair-t", "{question}" }
            div { class: "dui-pair-s", "Same pictures, same order: then it's really yours." }
            div {
                class: "dui-sas",
                role: "img",
                "aria-label": read_out.join(", "),
                for (index , picture) in pictures.into_iter().enumerate() {
                    div {
                        key: "{index}-{picture.name}",
                        class: "dui-sas-i",
                        style: "--i:{index}",
                        "aria-hidden": "true",
                        span { "{picture.emoji}" }
                        small { "{picture.name}" }
                    }
                }
            }
            div {
                class: "dui-sas-digits",
                "data-tip": "The same code, for a Dirt that shows digits",
                "{code.digits()}"
            }
            div { class: "dui-pair-a",
                Btn {
                    kind: "ghost",
                    disabled: answered,
                    onclick: move |_| on_differ.call(()),
                    "They don't"
                }
                button {
                    r#type: "button",
                    class: "dui-btn primary",
                    "data-dui-gather": "",
                    disabled: answered,
                    // The code is the one drawn with this button, taken as
                    // it is pressed.
                    onclick: move |_| on_match.call(code),
                    IconSvg { icon: Icon::Check, size: IconSize::Small }
                    "They match"
                }
            }
        }
    }
}

/// The end of a comparison: the drawn tick, or the red cross.
#[component]
pub(super) fn Outcome(
    good: bool,
    title: String,
    detail: String,
    #[props(default)] children: Element,
) -> Element {
    rsx! {
        div { class: "dui-pair", role: "status",
            div { class: if good { "dui-big-check" } else { "dui-big-check no" },
                IconSvg { icon: if good { Icon::Check } else { Icon::X } }
            }
            div { class: "dui-pair-t", "{title}" }
            div { class: "dui-pair-s", "{detail}" }
            {children}
        }
    }
}
