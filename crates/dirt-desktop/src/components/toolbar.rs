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
        let db = state.db_service.read().clone();
        let mut notes = state.notes;
        let mut current_note_id = state.current_note_id;

        spawn(async move {
            if let Some(db) = db {
                match db.create_note("").await {
                    Ok(note) => {
                        tracing::info!("Created new note: {}", note.id);
                        // Add to notes list and select the new note
                        let note_id = note.id;
                        notes.write().insert(0, note);
                        current_note_id.set(Some(note_id));
                    }
                    Err(e) => {
                        tracing::error!("Failed to create note: {}", e);
                    }
                }
            }
        });
    };

    let delete_note = move |_| {
        let note_id = *state.current_note_id.read();
        if let Some(id) = note_id {
            let db = state.db_service.read().clone();
            let mut notes = state.notes;
            let mut current_note_id = state.current_note_id;

            spawn(async move {
                if let Some(db) = db {
                    match db.delete_note(&id).await {
                        Ok(()) => {
                            tracing::info!("Deleted note: {}", id);
                            // Remove from notes list
                            notes.write().retain(|n| n.id != id);
                            // Clear selection
                            current_note_id.set(None);
                        }
                        Err(e) => {
                            tracing::error!("Failed to delete note: {}", e);
                        }
                    }
                }
            });
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
