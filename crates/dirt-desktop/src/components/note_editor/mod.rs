//! Note editor component

use std::time::Duration;

use dioxus::prelude::*;

use dirt_core::NoteId;

use self::attachment_panel::AttachmentPanel;
use crate::queries::invalidate_notes_query;
use crate::state::AppState;

mod attachment_panel;
mod attachment_preview;
mod attachment_utils;
mod transcription;

/// Idle save delay - save after 2 seconds of no typing
const IDLE_SAVE_MS: u64 = 2000;

/// Plain text note editor with auto-save
#[component]
pub fn NoteEditor() -> Element {
    let mut state = use_context::<AppState>();
    let current_note = state.current_note();
    let colors = (state.theme)().palette();

    // Local editor state for the selected note.
    let mut content = use_signal(String::new);
    let mut current_note_id = use_signal(|| None::<NoteId>);

    // Version-based save tracking to debounce writes.
    let mut save_version = use_signal(|| 0u64);
    let mut last_saved_version = use_signal(|| 0u64);

    // Sync content when selected note changes.
    use_effect(move || {
        let selected = state.current_note();
        let selected_id = selected.as_ref().map(|note| note.id);

        if selected_id != current_note_id() {
            if let Some(note) = selected {
                content.set(note.content);
            } else {
                content.set(String::new());
            }
            current_note_id.set(selected_id);
            save_version.set(0);
            last_saved_version.set(0);
        }
    });

    // Debounced auto-save.
    use_effect(move || {
        let current_version = save_version();
        if current_version == 0 || current_version == last_saved_version() {
            return;
        }

        let note_id = current_note_id();
        let content_to_save = content();

        if let Some(id) = note_id {
            state.enqueue_pending_change(id);
        }

        spawn(async move {
            tokio::time::sleep(Duration::from_millis(IDLE_SAVE_MS)).await;

            if save_version() != current_version {
                return;
            }

            if let Some(id) = note_id {
                let db = state.db_service.read().clone();
                if let Some(db) = db {
                    match db.update_note(&id, &content_to_save).await {
                        Ok(_) => {
                            tracing::debug!("Auto-saved note: {}", id);
                            last_saved_version.set(current_version);
                            invalidate_notes_query().await;
                        }
                        Err(error) => {
                            tracing::error!("Failed to save note: {}", error);
                        }
                    }
                }
            }
        });
    });

    let mut perform_save_now = move || {
        let current_version = save_version();
        if current_version == 0 || current_version == last_saved_version() {
            return;
        }

        let note_id = current_note_id();
        let content_to_save = content();

        if let Some(id) = note_id {
            state.enqueue_pending_change(id);
        }

        spawn(async move {
            if let Some(id) = note_id {
                let db = state.db_service.read().clone();
                if let Some(db) = db {
                    match db.update_note(&id, &content_to_save).await {
                        Ok(_) => {
                            tracing::debug!("Saved note on blur/shortcut: {}", id);
                            last_saved_version.set(current_version);
                            invalidate_notes_query().await;
                        }
                        Err(error) => {
                            tracing::error!("Failed to save note: {}", error);
                        }
                    }
                }
            }
        });
    };

    let on_input = move |evt: Event<FormData>| {
        let new_content = evt.value();
        content.set(new_content.clone());
        save_version.set(save_version() + 1);

        // Optimistically reflect the latest content in local list state.
        if let Some(id) = current_note_id() {
            let mut notes = state.notes.write();
            if let Some(note) = notes.iter_mut().find(|note| note.id == id) {
                note.content = new_content;
                note.updated_at = chrono::Utc::now().timestamp_millis();
            }
        }
    };

    let on_blur = move |_| {
        perform_save_now();
    };

    let on_keydown = move |evt: Event<KeyboardData>| {
        if evt.modifiers().ctrl() && evt.key() == Key::Character("s".to_string()) {
            evt.prevent_default();
            perform_save_now();
        }
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
                    onblur: on_blur,
                    onkeydown: on_keydown,
                }

                AttachmentPanel {
                    note_id: current_note_id(),
                    editor_content: content(),
                    on_editor_content_change: move |updated_content: String| {
                        content.set(updated_content.clone());
                        if let Some(id) = current_note_id() {
                            let mut notes = state.notes.write();
                            if let Some(note) = notes.iter_mut().find(|note| note.id == id) {
                                note.content = updated_content;
                                note.updated_at = chrono::Utc::now().timestamp_millis();
                            }
                        }
                    },
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
