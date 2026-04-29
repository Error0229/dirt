//! Note editor component
//!
//! Attachment UI was removed during the Supabase teardown — the
//! `AttachmentPanel` from main relied on `media_api_client` and
//! `auth_session`, both of which are gone with the R2/Supabase work
//! that's deferred to a future commit. The plain-text editor stays.

use std::time::Duration;

use dioxus::prelude::*;

use dirt_core::NoteId;

use crate::components::update_note_content;
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

    // Shared save logic used by both debounced auto-save and immediate save.
    let mut persist_note = move |version: u64, note_id: Option<NoteId>, content_to_save: String| {
        if let Some(id) = note_id {
            state.enqueue_pending_change(id);
        }

        let db = state.db_service.read().clone();
        spawn(async move {
            if let Some(id) = note_id {
                if let Some(db) = db {
                    match db.update_note(&id, &content_to_save).await {
                        Ok(_) => {
                            tracing::debug!("Saved note: {}", id);
                            last_saved_version.set(version);
                            invalidate_notes_query().await;
                            state.trigger_sync();
                        }
                        Err(error) => {
                            tracing::error!("Failed to save note: {}", error);
                        }
                    }
                }
            }
        });
    };

    // Debounced auto-save.
    use_effect(move || {
        let current_version = save_version();
        if current_version == 0 || current_version == last_saved_version() {
            return;
        }

        let note_id = current_note_id();
        let content_to_save = content();
        let mut persist = persist_note;

        spawn(async move {
            tokio::time::sleep(Duration::from_millis(IDLE_SAVE_MS)).await;

            if save_version() != current_version || last_saved_version() == current_version {
                return;
            }

            persist(current_version, note_id, content_to_save);
        });
    });

    let mut perform_save_now = move || {
        let current_version = save_version();
        if current_version == 0 || current_version == last_saved_version() {
            return;
        }
        persist_note(current_version, current_note_id(), content());
    };

    let on_input = move |evt: Event<FormData>| {
        let new_content = evt.value();
        content.set(new_content.clone());
        save_version.set(save_version() + 1);

        // Optimistically reflect the latest content in local list state.
        if let Some(id) = current_note_id() {
            update_note_content(&mut state, id, new_content);
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
                background: {colors.bg_primary};
                position: relative;
                min-width: 0;
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
                        line-height: 1.65;
                        background: transparent;
                        color: {colors.text_primary};
                        padding: 20px 32px;
                        box-sizing: border-box;
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
                        font-size: 14px;
                    ",
                    "Select a note or press Ctrl+N"
                }
            }
        }
    }
}
