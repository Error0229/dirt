//! Pulling a list down to sync it (interface design, sections 5.5, 8.2 and
//! 12).
//!
//! The list's content follows the finger with resistance while the list is
//! at its top. Let go past the mark, it holds while an arrow spins above
//! it, and then settles back. All of that is in the script. Rust is told
//! once, when the list is let go past the mark, and says for how long the
//! arrow spins with `busy`. The sync status is the button for the same
//! thing.

use dioxus::prelude::*;

use crate::events::{use_outcome, Outcome};
use crate::icons::{Icon, IconSvg};

/// The content of a list that scrolls, which can be pulled down. Put it
/// inside the [`crate::components::ScrollArea`] of the list, around what
/// the list shows.
#[component]
pub fn PullToSync(
    /// Sync is running: the arrow spins for as long as it is.
    busy: bool,
    /// The list was pulled down past the mark and let go.
    on_pull: EventHandler<()>,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::Pull {
            on_pull.call(());
        }
    });
    rsx! {
        div {
            class: "dui-pull {class}",
            "data-dui-pull": "",
            "data-dui-id": "{id}",
            "data-busy": busy.then_some(""),
            div { class: "dui-ptr", "aria-hidden": "true",
                IconSvg { icon: Icon::Refresh }
            }
            {children}
        }
    }
}
