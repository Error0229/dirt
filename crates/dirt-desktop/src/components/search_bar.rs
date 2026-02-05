//! Search bar component

use dioxus::prelude::*;

use super::input::Input;
use crate::state::AppState;

/// Search bar for filtering notes
#[component]
pub fn SearchBar() -> Element {
    let mut state = use_context::<AppState>();

    rsx! {
        div {
            class: "search-bar",

            Input {
                r#type: "text",
                placeholder: "Search notes...",
                value: "{state.search_query}",
                oninput: move |evt: FormEvent| {
                    state.search_query.set(evt.value());
                },
            }
        }
    }
}
