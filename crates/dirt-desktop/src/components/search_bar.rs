//! Search bar component

use dioxus::prelude::*;

use crate::state::AppState;

/// Search bar for filtering notes
#[component]
pub fn SearchBar() -> Element {
    let mut state = use_context::<AppState>();

    rsx! {
        div {
            class: "search-bar",
            style: "padding: 12px 16px; border-bottom: 1px solid var(--border-color, #e0e0e0);",

            input {
                r#type: "text",
                placeholder: "Search notes...",
                value: "{state.search_query}",
                oninput: move |evt| {
                    state.search_query.set(evt.value());
                },
                style: "width: 100%; padding: 8px 12px; border: 1px solid var(--border-color, #e0e0e0); border-radius: 6px; font-size: 14px;",
            }
        }
    }
}
