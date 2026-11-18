//! The app's own menu on a right click (interface design, section 7.4):
//! Copy, Paste, Select all, and whatever the screen adds.
//!
//! A release build turns the web view's menu off, since it is a browser's
//! and offers reloading the page. This one takes its place wherever the
//! new interface is drawn.
//!
//! The script shows it where the pointer is, or by the field for the menu
//! key, and offers what applies: Copy when something is selected, Paste
//! and Select all in a text field. Copy and Select all are done in the
//! page. Paste needs the clipboard, which a page may not read without
//! asking, so the script tells Rust ([`Outcome::Paste`]) and the app
//! hands the text back with [`crate::dom::paste`].

use dioxus::prelude::*;

use crate::events::{use_outcome, Outcome};
use crate::icons::{Icon, IconSvg};

/// The menu. One is enough for a screen; it is always in the page and
/// shows when asked for. `children` are the screen's own entries, each a
/// [`MenuItem`], shown under the three every text has.
#[component]
pub fn ContextMenu(
    /// Paste was chosen: read the clipboard and call [`crate::dom::paste`].
    on_paste: EventHandler<()>,
    children: Element,
) -> Element {
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::Paste {
            on_paste.call(());
        }
    });
    rsx! {
        div {
            class: "dui-menu",
            role: "menu",
            "aria-label": "Text",
            "data-dui-menu": "",
            "data-dui-id": "{id}",
            "data-dui-arrows": "",
            tabindex: "-1",
            button {
                r#type: "button",
                role: "menuitem",
                "data-dui-do": "copy",
                tabindex: "-1",
                IconSvg { icon: Icon::Copy }
                "Copy"
            }
            button {
                r#type: "button",
                role: "menuitem",
                "data-dui-do": "paste",
                tabindex: "-1",
                IconSvg { icon: Icon::File }
                "Paste"
            }
            button {
                r#type: "button",
                role: "menuitem",
                "data-dui-do": "all",
                tabindex: "-1",
                IconSvg { icon: Icon::Check }
                "Select all"
            }
            {children}
        }
    }
}

/// An entry a screen adds to the menu. `shortcut` is shown as key chips,
/// its keys separated by spaces.
#[component]
pub fn MenuItem(
    icon: Icon,
    label: String,
    #[props(default)] shortcut: Option<String>,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let named = shortcut.as_deref().map(|keys| keys.replace(' ', "+"));
    rsx! {
        button {
            r#type: "button",
            role: "menuitem",
            tabindex: "-1",
            "aria-keyshortcuts": named,
            onclick: move |event| onclick.call(event),
            IconSvg { icon }
            "{label}"
            if let Some(keys) = shortcut {
                span { class: "dui-menu-keys", "aria-hidden": "true",
                    for key in keys.split(' ') {
                        kbd { key: "{key}", "{key}" }
                    }
                }
            }
        }
    }
}
