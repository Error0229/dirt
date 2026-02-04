//! Home view - main application screen

use dioxus::prelude::*;

use crate::components::{NoteEditor, NoteList, SearchBar, Sidebar};
use crate::state::AppState;

/// Home view component - the main application screen
#[component]
pub fn Home() -> Element {
    let _state = use_context::<AppState>();

    rsx! {
        div {
            class: "home-container",
            style: "display: flex; height: 100vh;",

            Sidebar {}

            div {
                class: "main-content",
                style: "flex: 1; display: flex; flex-direction: column;",

                SearchBar {}

                div {
                    class: "content-area",
                    style: "flex: 1; display: flex; overflow: hidden;",

                    NoteList {}
                    NoteEditor {}
                }
            }
        }
    }
}
