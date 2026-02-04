//! Note editor component

use dioxus::prelude::*;

use crate::state::AppState;

/// Plain text note editor
#[component]
pub fn NoteEditor() -> Element {
    let state = use_context::<AppState>();
    let current_note = state.current_note();

    rsx! {
        div {
            class: "note-editor",
            style: "flex: 1; display: flex; flex-direction: column; padding: 16px;",

            if let Some(note) = current_note {
                textarea {
                    class: "editor-textarea",
                    style: "flex: 1; width: 100%; border: none; outline: none; resize: none; font-family: inherit; font-size: 14px; line-height: 1.6;",
                    value: "{note.content}",
                    placeholder: "Start typing...",
                    // TODO: Implement save on change with debounce
                }
            } else {
                div {
                    class: "editor-placeholder",
                    style: "flex: 1; display: flex; align-items: center; justify-content: center; color: var(--text-secondary, #666);",
                    "Select a note or create a new one"
                }
            }
        }
    }
}
