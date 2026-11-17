//! The banner (interface design, sections 5.5 and 6.8).
//!
//! It is for something another device is waiting on, which cannot wait for
//! the person to open Settings. It drops from the top edge with a bounce
//! and stays until it is answered or dismissed.

use dioxus::prelude::*;

use crate::appearance::Platform;
use crate::components::button::{Button, ButtonKind};
use crate::components::icon_button::{IconButton, IconButtonSize};
use crate::icons::{Icon, IconSvg};

/// A banner at the top of the app. It is always in the page, so that it
/// can move in and out; `open` says whether it shows.
#[component]
pub fn Banner(
    open: bool,
    /// What is waiting: "Work laptop wants to join".
    title: String,
    /// One quieter line under it.
    #[props(default)]
    detail: Option<String>,
    #[props(default = Icon::Laptop)] icon: Icon,
    /// The word on its one button: "Review".
    action: String,
    on_action: EventHandler<()>,
    /// The close icon's name. Only the desktop shows a close icon.
    #[props(default = "Not now".to_owned())]
    dismiss_label: String,
    on_dismiss: EventHandler<()>,
    #[props(default = Platform::current())] platform: Platform,
) -> Element {
    rsx! {
        div {
            class: if open { "dui-banner on" } else { "dui-banner" },
            role: "status",
            "aria-live": "polite",
            // Out of the tab order and unseen by a screen reader while away.
            "inert": if open { None } else { Some("") },
            span { class: "dui-banner-icon", IconSvg { icon } }
            div { class: "dui-banner-words",
                div { class: "dui-banner-title", "{title}" }
                if let Some(detail) = detail {
                    div { class: "dui-banner-detail", "{detail}" }
                }
            }
            Button {
                kind: ButtonKind::Primary,
                small: true,
                onclick: move |_| on_action.call(()),
                "{action}"
            }
            if platform == Platform::Desktop {
                IconButton {
                    icon: Icon::X,
                    label: dismiss_label,
                    size: IconButtonSize::Small,
                    onclick: move |_| on_dismiss.call(()),
                }
            }
        }
    }
}
