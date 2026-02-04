//! Toolbar component with actions

use dioxus::prelude::*;

use crate::state::AppState;

/// Toolbar with action buttons
#[component]
pub fn Toolbar() -> Element {
    let mut state = use_context::<AppState>();
    let has_selected_note = state.current_note().is_some();

    let create_note = move |_| {
        if let Some(ref db) = *state.db_service.read() {
            match db.create_note("") {
                Ok(note) => {
                    tracing::info!("Created new note: {}", note.id);
                    // Add to notes list
                    let mut notes = state.notes.write();
                    notes.insert(0, note.clone());
                    // Select the new note
                    state.current_note_id.set(Some(note.id));
                }
                Err(e) => {
                    tracing::error!("Failed to create note: {}", e);
                }
            }
        }
    };

    let delete_note = move |_| {
        let note_id = *state.current_note_id.read();
        if let Some(id) = note_id {
            if let Some(ref db) = *state.db_service.read() {
                match db.delete_note(&id) {
                    Ok(()) => {
                        tracing::info!("Deleted note: {}", id);
                        // Remove from notes list
                        let mut notes = state.notes.write();
                        notes.retain(|n| n.id != id);
                        // Clear selection
                        state.current_note_id.set(None);
                    }
                    Err(e) => {
                        tracing::error!("Failed to delete note: {}", e);
                    }
                }
            }
        }
    };

    rsx! {
        div {
            class: "toolbar",
            style: "display: flex; align-items: center; gap: 8px; padding: 8px 16px; border-bottom: 1px solid var(--border-color, #e0e0e0); background: var(--toolbar-bg, #fafafa);",

            button {
                class: "btn btn-primary",
                style: "display: flex; align-items: center; gap: 4px; padding: 6px 12px; background: #2563eb; color: white; border: none; border-radius: 6px; cursor: pointer; font-size: 13px; font-weight: 500;",
                onclick: create_note,
                "+ New Note"
            }

            if has_selected_note {
                button {
                    class: "btn btn-danger",
                    style: "display: flex; align-items: center; gap: 4px; padding: 6px 12px; background: #dc2626; color: white; border: none; border-radius: 6px; cursor: pointer; font-size: 13px; font-weight: 500;",
                    onclick: delete_note,
                    "Delete"
                }
            }
        }
    }
}
