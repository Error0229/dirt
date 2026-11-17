//! A card that can be swiped away (interface design, sections 5.5 and
//! 8.2). The card follows the finger exactly; let go past 96 px it flies
//! off, let go before it springs back.
//!
//! All of that happens in the script. Rust is told once, when the card has
//! flown off, and then takes the card out of its list; the list's
//! animation closes the gap. Every gesture has a button too (section 12):
//! what a swipe does must also be offered by a control.

use dioxus::prelude::*;

use crate::events::{use_outcome, Outcome};
use crate::icons::{Icon, IconSvg};

/// A card that can be swiped to the left.
#[component]
pub fn Swipeable(
    /// What is shown behind the card as it moves: what the swipe will do.
    #[props(default = Icon::Archive)]
    icon: Icon,
    /// The swipe deletes: the background is red.
    #[props(default)]
    danger: bool,
    /// The card has been swiped away.
    on_swipe: EventHandler<()>,
    children: Element,
) -> Element {
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::Swipe {
            on_swipe.call(());
        }
    });
    rsx! {
        div { class: "dui-swipe",
            div {
                class: if danger { "dui-swipe-bg danger" } else { "dui-swipe-bg" },
                "aria-hidden": "true",
                IconSvg { icon }
            }
            div { class: "dui-swipe-in", "data-dui-swipe": "", "data-dui-id": "{id}", {children} }
        }
    }
}
