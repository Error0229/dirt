//! Note editor component

use std::time::Duration;

use dioxus::prelude::*;

use dirt_core::NoteId;

use crate::state::AppState;

/// Debounce delay for auto-save (in milliseconds)
const SAVE_DEBOUNCE_MS: u64 = 500;

/// Plain text note editor with auto-save
#[component]
pub fn NoteEditor() -> Element {
    let mut state = use_context::<AppState>();
    let current_note = state.current_note();
    let colors = (state.theme)().palette();

    // Local state for the editor content
    let mut content = use_signal(String::new);
    let mut last_note_id = use_signal(|| None::<NoteId>);
    let mut is_dirty = use_signal(|| false);

    // Sync content when selected note changes
    use_effect(move || {
        let current = state.current_note();
        let current_id = current.as_ref().map(|n| n.id);

        if current_id != *last_note_id.read() {
            if let Some(note) = current {
                content.set(note.content);
            } else {
                content.set(String::new());
            }
            last_note_id.set(current_id);
            is_dirty.set(false);
        }
    });

    // Auto-save with debounce
    use_effect(move || {
        if !*is_dirty.read() {
            return;
        }

        let note_id = *last_note_id.read();
        let content_to_save = content.read().clone();

        spawn(async move {
            // Wait for debounce period
            tokio::time::sleep(Duration::from_millis(SAVE_DEBOUNCE_MS)).await;

            // Check if still dirty (user might have typed more)
            if !*is_dirty.read() {
                return;
            }

            if let Some(id) = note_id {
                let db = state.db_service.read().clone();
                if let Some(db) = db {
                    match db.update_note(&id, &content_to_save).await {
                        Ok(updated_note) => {
                            tracing::debug!("Auto-saved note: {}", id);
                            // Update the note in the global state
                            let mut notes = state.notes.write();
                            if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
                                *note = updated_note;
                            }
                            is_dirty.set(false);
                        }
                        Err(e) => {
                            tracing::error!("Failed to save note: {}", e);
                        }
                    }
                }
            }
        });
    });

    let on_input = move |evt: Event<FormData>| {
        content.set(evt.value());
        is_dirty.set(true);
    };

    rsx! {
        div {
            class: "note-editor",
            style: "
                flex: 1;
                display: flex;
                flex-direction: column;
                padding: 16px;
                background: {colors.bg_primary};
            ",

            if current_note.is_some() {
                textarea {
                    class: "editor-textarea",
                    style: "
                        flex: 1;
                        width: 100%;
                        border: none;
                        outline: none;
                        resize: none;
                        font-family: inherit;
                        font-size: inherit;
                        line-height: 1.6;
                        background: transparent;
                        color: {colors.text_primary};
                    ",
                    value: "{content}",
                    placeholder: "Start typing...",
                    oninput: on_input,
                }
            } else {
                div {
                    class: "editor-placeholder",
                    style: "
                        flex: 1;
                        display: flex;
                        align-items: center;
                        justify-content: center;
                        color: {colors.text_muted};
                    ",
                    "Select a note or create a new one"
                }
            }
        }
    }
}
