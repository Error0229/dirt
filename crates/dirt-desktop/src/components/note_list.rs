//! Note list component

use dioxus::prelude::*;

use super::NoteCard;
use crate::state::AppState;

/// List of notes with previews
#[component]
pub fn NoteList() -> Element {
    let mut state = use_context::<AppState>();
    let filtered_notes = state.filtered_notes();
    let current_id = (state.current_note_id)();
    let colors = (state.theme)().palette();

    rsx! {
        div {
            class: "note-list",
            style: "
                width: 280px;
                border-right: 1px solid {colors.border};
                overflow-y: auto;
                background: {colors.bg_primary};
            ",

            if filtered_notes.is_empty() {
                div {
                    style: "
                        padding: 20px;
                        text-align: center;
                        color: {colors.text_muted};
                    ",
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
                            NoteCard {
                                key: "{note_id}",
                                title,
                                preview,
                                is_selected,
                                onclick: move |_| {
                                    state.current_note_id.set(Some(note_id));
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}
