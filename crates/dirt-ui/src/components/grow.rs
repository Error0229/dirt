//! A page that grows out of what opened it (interface design, sections 3,
//! 5.5 and 10).
//!
//! Android's note grows out of its card and shrinks back into it, and the
//! new page grows out of the bar at the bottom.
//!
//! The page is always in the screen, so that it can move in and out;
//! `open` says which way. The script sees the class change and plays the
//! movement from the element named by `from`, as it is on screen then. It
//! is told nothing else, and tells Rust nothing but Android's Back, which
//! the page takes while it is open.

use dioxus::prelude::*;

use crate::events::{use_outcome, Outcome};

/// What a page grows out of and shrinks back into: the ID of an element in
/// the page, and what kind of element it is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GrowFrom {
    /// A note's card ([`crate::components::card_element_id`]): 440 ms out
    /// of it, 340 ms back.
    Card(String),
    /// The bar a new page is asked for from: the glide curve over 560 ms
    /// out of it, 320 ms back.
    Bar(String),
    /// A new page let go with words on it (section 5.5): it closes to a
    /// 7 px circle where its words began, and the grain of the card named
    /// here (marked `data-dui-settle`) is tossed from there onto it.
    Grain(String),
    /// Nothing on screen: the page fades.
    #[default]
    Nowhere,
}

impl GrowFrom {
    fn parts(&self) -> (Option<&str>, Option<&'static str>) {
        match self {
            Self::Card(id) => (Some(id), Some("card")),
            Self::Bar(id) => (Some(id), Some("bar")),
            Self::Grain(id) => (Some(id), Some("grain")),
            Self::Nowhere => (None, None),
        }
    }
}

/// A page over the whole screen. Inside it, the element with
/// `data-dui-grow-bar` (its top bar) fades in after it, and the element
/// with `data-dui-grow-body` (its text) rises in behind it.
#[component]
pub fn GrowPage(
    open: bool,
    /// What it grows out of as it opens, and goes back into as it closes.
    from: GrowFrom,
    /// Its name for a screen reader.
    label: String,
    /// Android's Back, while it is open.
    on_back: EventHandler<()>,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::Back {
            on_back.call(());
        }
    });
    let (from_id, kind) = from.parts();
    rsx! {
        section {
            class: if open { "dui-grow on {class}" } else { "dui-grow {class}" },
            role: "dialog",
            "aria-label": "{label}",
            "aria-hidden": if open { None } else { Some("true") },
            "inert": if open { None } else { Some("") },
            "data-dui-grow": "",
            "data-dui-from": from_id,
            "data-dui-from-kind": kind,
            "data-dui-id": "{id}",
            "data-dui-back": open.then_some(""),
            {children}
        }
    }
}
