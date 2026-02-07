//! Note editor component

use std::time::Duration;

use dioxus::prelude::*;

use dirt_core::NoteId;

use crate::queries::invalidate_notes_query;
use crate::state::AppState;

/// Idle save delay - save after 2 seconds of no typing
const IDLE_SAVE_MS: u64 = 2000;

/// Plain text note editor with auto-save
#[component]
pub fn NoteEditor() -> Element {
    let mut state = use_context::<AppState>();
    let current_note = state.current_note();
    let colors = (state.theme)().palette();

    // Local state for the editor content
    let mut content = use_signal(String::new);
    let mut last_note_id = use_signal(|| None::<NoteId>);

    // Version-based save tracking
    let mut save_version = use_signal(|| 0u64);
    let mut last_saved_version = use_signal(|| 0u64);

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
            // Reset save tracking for new note
            save_version.set(0);
            last_saved_version.set(0);
        }
    });

    // Auto-save with proper debounce using version tracking
    use_effect(move || {
        let current_version = save_version();
        if current_version == 0 || current_version == last_saved_version() {
            return; // Nothing to save
        }

        let note_id = *last_note_id.read();
        let content_to_save = content.read().clone();

        spawn(async move {
            // Wait for idle period
            tokio::time::sleep(Duration::from_millis(IDLE_SAVE_MS)).await;

            // Check if version changed during sleep (user typed more)
            if save_version() != current_version {
                return; // Stale, a newer version is pending
            }

            // Perform save to DB
            if let Some(id) = note_id {
                let db = state.db_service.read().clone();
                if let Some(db) = db {
                    match db.update_note(&id, &content_to_save).await {
                        Ok(_) => {
                            tracing::debug!("Auto-saved note: {}", id);
                            last_saved_version.set(current_version);
                            // Invalidate query to keep other views in sync
                            invalidate_notes_query().await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to save note: {}", e);
                        }
                    }
                }
            }
        });
    });

    // Helper to perform immediate save
    let perform_save_now = move || {
        let current_version = save_version();
        if current_version == 0 || current_version == last_saved_version() {
            return; // Nothing to save
        }

        let note_id = *last_note_id.read();
        let content_to_save = content.read().clone();

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
                        Err(e) => {
                            tracing::error!("Failed to save note: {}", e);
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

        // Optimistic update: update local state immediately
        if let Some(id) = *last_note_id.read() {
            let mut notes = state.notes.write();
            if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
                note.content = new_content;
                note.updated_at = chrono::Utc::now().timestamp_millis();
            }
        }
    };

    let on_blur = move |_| {
        perform_save_now();
    };

    let on_keydown = move |evt: Event<KeyboardData>| {
        // Ctrl+S to save immediately
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
