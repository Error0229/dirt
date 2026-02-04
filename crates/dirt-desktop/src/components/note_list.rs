//! Note list component

use dioxus::prelude::*;

use crate::state::AppState;

/// List of notes with previews
#[component]
pub fn NoteList() -> Element {
    let mut state = use_context::<AppState>();
    let filtered_notes = state.filtered_notes();
    let current_id = (state.current_note_id)();

    rsx! {
        div {
            class: "note-list",
            style: "width: 280px; border-right: 1px solid var(--border-color, #e0e0e0); overflow-y: auto;",

            if filtered_notes.is_empty() {
                div {
                    style: "padding: 20px; text-align: center; color: var(--text-secondary, #666);",
                    "No notes yet"
                }
            } else {
                for note in filtered_notes {
                    {
                        let note_id = note.id;
                        let is_selected = current_id == Some(note_id);
                        let title = note.title_preview(40);
                        let preview = note.title_preview(60);

                        rsx! {
                            div {
                                class: if is_selected { "note-item selected" } else { "note-item" },
                                style: "padding: 12px 16px; border-bottom: 1px solid var(--border-color, #e0e0e0); cursor: pointer;",
                                onclick: move |_| {
                                    state.current_note_id.set(Some(note_id));
                                },

                                div {
                                    class: "note-title",
                                    style: "font-weight: 500; margin-bottom: 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                    "{title}"
                                }

                                div {
                                    class: "note-preview",
                                    style: "font-size: 12px; color: var(--text-secondary, #666); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                    "{preview}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
