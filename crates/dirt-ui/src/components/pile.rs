//! The pile under the list (interface design, section 6.17): the heap of
//! sand, one grain for every thought, with how many there are and since
//! when, and how many of them are archived, which lists them.
//!
//! It sits at the end of a list, as one of its rows (`data-key="pile"`), so
//! that it moves with the rows above it. The heap is worked out once each
//! time the notes change ([`crate::pile::Heap::of`]); as a new thought
//! settles onto the list the script drops its grain onto the heap, if the
//! heap is in sight.

use std::sync::Arc;

use dioxus::prelude::*;
use dirt_core::models::PileSummary;

use crate::pile::{grouped, line, thoughts, Heap};

/// The pile: the heap, "1,254 thoughts since Nov 2025" under it, and "3
/// archived" if any are.
#[component]
pub fn Pile(
    heap: Arc<Heap>,
    summary: PileSummary,
    /// "3 archived" was pressed: the list shows the archived notes.
    on_archived: EventHandler<()>,
) -> Element {
    let Some(words) = line(&summary) else {
        return rsx! {};
    };
    let tip = thoughts(summary.thoughts);
    let archived =
        (summary.archived > 0).then(|| format!("{} archived", grouped(summary.archived)));
    rsx! {
        div { class: "dui-pile", role: "presentation", "data-key": "pile", "data-dui-pile": "",
            if heap.grains > 0 {
                div {
                    class: "dui-pile-heap",
                    role: "img",
                    "aria-label": "{tip}",
                    "data-tip": "{tip}",
                    dangerous_inner_html: "{heap.svg}",
                }
            }
            div { class: "dui-pile-line", "{words}" }
            if let Some(archived) = archived {
                button {
                    r#type: "button",
                    class: "dui-pile-arch",
                    onclick: move |_| on_archived.call(()),
                    "{archived}"
                }
            }
        }
    }
}
