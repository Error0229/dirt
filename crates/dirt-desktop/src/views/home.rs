//! Home view - main application screen

use dioxus::prelude::*;

use crate::components::{create_note_optimistic, NoteEditor, NoteList, SearchBar, Sidebar, Toolbar};
use crate::state::AppState;

/// Home view component - the main application screen
#[component]
pub fn Home() -> Element {
    let mut state = use_context::<AppState>();

    let handle_keydown = move |evt: Event<KeyboardData>| {
        let is_new_note_shortcut = (evt.modifiers().ctrl() || evt.modifiers().meta())
            && matches!(
                evt.key(),
                Key::Character(ch) if ch.eq_ignore_ascii_case("n")
            );

        if is_new_note_shortcut {
            evt.prevent_default();
            create_note_optimistic(&mut state);
            return;
        }

        if evt.key() == Key::Escape {
            if (state.settings_open)() {
                state.settings_open.set(false);
                return;
            }
            if (state.current_note_id)().is_some() {
                state.current_note_id.set(None);
            }
        }
    };

    rsx! {
        div {
            class: "home-container",
            style: "display: flex; height: 100vh;",
            onkeydown: handle_keydown,

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
