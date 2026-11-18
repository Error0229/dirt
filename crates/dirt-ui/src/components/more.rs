//! More rows as a list nears its end (interface design, section 3: "The
//! list loads more as it nears the end. No button").

use dioxus::prelude::*;

use crate::events::{use_outcome, Outcome};

/// A mark at the end of a list that has more to show. When scrolling
/// brings it near, `on_more` is called once.
///
/// Put it last inside a [`crate::components::ScrollArea`], only while
/// there is more, and give it a `key` that changes when more has been
/// shown: a new mark is watched afresh, so a list still short of its
/// window asks again.
#[component]
pub fn LoadMore(on_more: EventHandler<()>) -> Element {
    let id = use_outcome(move |outcome| {
        if outcome == Outcome::More {
            on_more.call(());
        }
    });
    rsx! {
        div {
            class: "dui-more",
            "aria-hidden": "true",
            "data-dui-more": "",
            "data-dui-id": "{id}",
        }
    }
}
