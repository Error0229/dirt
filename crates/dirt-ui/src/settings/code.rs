//! The code comparison (interface design, sections 5.5 and 6.13): seven
//! pictures with their names, in a row, the same on both devices.
//!
//! The pictures pop in one after another. When **They match** is pressed
//! they gather to the middle and the tick takes their place; the script
//! plays that (`data-dui-gather`), and the answer reaches the exchange as
//! it ends.

use dioxus::prelude::*;
use dirt_core::enrolment::ShortCode;

use super::rows::Btn;
use crate::icons::{Icon, IconSize, IconSvg};

/// How long the pictures take to gather before the tick is drawn.
pub(super) const GATHER_MS: u64 = 400;

/// The code, and the two answers to it.
#[component]
pub fn CodeComparison(
    code: ShortCode,
    /// "Does Work laptop show these?"
    question: String,
    on_match: EventHandler<()>,
    on_differ: EventHandler<()>,
) -> Element {
    let pictures = code.pictures();
    let read_out: Vec<&str> = pictures.iter().map(|picture| picture.name).collect();
    rsx! {
        div { class: "dui-pair",
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
            div { class: "dui-pair-a",
                Btn { kind: "ghost", onclick: move |_| on_differ.call(()), "They don't" }
                button {
                    r#type: "button",
                    class: "dui-btn primary",
                    "data-dui-gather": "",
                    onclick: move |_| on_match.call(()),
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
