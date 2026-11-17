//! The panel (desktop) and the sheet (Android) (interface design, sections
//! 5.5, 6.9 and 12): what Settings is drawn in.
//!
//! Both are dialogs. As one opens, focus moves into it; Tab stays inside;
//! Esc closes it; and focus goes back to what opened it. The panel grows
//! out of the button that opened it and shrinks back into it. The sheet
//! rises from the bottom and can be dragged shut by its top edge. The
//! script does all of that. It is told nothing: it sees the `on` class
//! come and go.
//!
//! Both are always in the page, so that they can move in and out; `open`
//! says which way. An element inside with `data-dui-rise` rises into place
//! after its neighbours as the panel or sheet opens.

use dioxus::prelude::*;

use crate::components::icon_button::IconButton;
use crate::events::{use_outcome, Outcome};
use crate::icons::Icon;

/// The dim behind a panel or sheet. A press on it closes.
#[component]
fn Scrim(open: bool, on_close: EventHandler<()>) -> Element {
    rsx! {
        div {
            class: if open { "dui-scrim on" } else { "dui-scrim" },
            "aria-hidden": "true",
            onclick: move |_| on_close.call(()),
        }
    }
}

/// A panel in the middle of the window, for the desktop. `label` is its
/// name for a screen reader.
#[component]
pub fn Panel(open: bool, label: String, on_close: EventHandler<()>, children: Element) -> Element {
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::Dismiss {
            on_close.call(());
        }
    });
    rsx! {
        Scrim { open, on_close }
        div {
            class: if open { "dui-panel on" } else { "dui-panel" },
            role: "dialog",
            "aria-modal": "true",
            "aria-label": "{label}",
            "data-dui-modal": "panel",
            "data-dui-id": "{id}",
            "inert": if open { None } else { Some("") },
            tabindex: "-1",
            IconButton {
                icon: Icon::X,
                label: "Close",
                shortcut: "Esc",
                class: "dui-panel-close",
                onclick: move |_| on_close.call(()),
            }
            div { class: "dui-panel-body", {children} }
        }
    }
}

/// A sheet from the bottom of the screen, for Android. Its title is at its
/// top, with a grabber: dragging that down closes the sheet.
#[component]
pub fn Sheet(open: bool, title: String, on_close: EventHandler<()>, children: Element) -> Element {
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::Dismiss {
            on_close.call(());
        }
    });
    rsx! {
        Scrim { open, on_close }
        div {
            class: if open { "dui-sheet on" } else { "dui-sheet" },
            role: "dialog",
            "aria-modal": "true",
            "aria-label": "{title}",
            "data-dui-modal": "sheet",
            "data-dui-id": "{id}",
            "inert": if open { None } else { Some("") },
            tabindex: "-1",
            div { class: "dui-sheet-head", "data-dui-sheet-grab": "",
                h2 { "{title}" }
                IconButton {
                    icon: Icon::X,
                    label: "Close",
                    onclick: move |_| on_close.call(()),
                }
            }
            div { class: "dui-sheet-body", {children} }
        }
    }
}
