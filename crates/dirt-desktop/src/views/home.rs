//! Home view - main application screen

use dioxus::prelude::*;

use crate::components::{NoteEditor, NoteList, SearchBar, Sidebar, Toolbar};

/// Home view component - the main application screen
#[component]
pub fn Home() -> Element {
    rsx! {
        div {
            class: "home-container",
            style: "display: flex; height: 100vh;",

            Sidebar {}

            div {
                class: "main-content",
                style: "flex: 1; display: flex; flex-direction: column;",

                Toolbar {}
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
