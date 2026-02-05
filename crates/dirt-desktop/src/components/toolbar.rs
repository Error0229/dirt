//! Toolbar component with actions

use dioxus::prelude::*;

use super::button::{Button, ButtonVariant};
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

    let open_settings = move |_| {
        state.settings_open.set(true);
    };

    rsx! {
        div {
            class: "toolbar",

            Button {
                variant: ButtonVariant::Primary,
                onclick: create_note,
                "+ New Note"
            }

            if has_selected_note {
                Button {
                    variant: ButtonVariant::Destructive,
                    onclick: delete_note,
                    "Delete"
                }
            }

            // Spacer
            div { style: "flex: 1;" }

            // Settings button
            Button {
                variant: ButtonVariant::Secondary,
                onclick: open_settings,
                "Settings"
            }
        }
    }
}
