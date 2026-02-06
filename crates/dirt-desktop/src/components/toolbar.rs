//! Toolbar component with actions

use dioxus::prelude::*;
use dirt_core::models::Note;

use super::button::{Button, ButtonVariant};
use crate::queries::invalidate_notes_query;
use crate::state::AppState;

/// Toolbar with action buttons
#[component]
pub fn Toolbar() -> Element {
    let mut state = use_context::<AppState>();
    let has_selected_note = state.current_note().is_some();

    let create_note = move |_| {
        // Create optimistic note with client-generated ID (UUID v7)
        let optimistic_note = Note::new("");
        let note_id = optimistic_note.id;

        // Update UI immediately (optimistic)
        state.notes.write().insert(0, optimistic_note.clone());
        state.current_note_id.set(Some(note_id));

        tracing::info!("Created new note (optimistic): {}", note_id);

        // Persist in background
        let db = state.db_service.read().clone();
        spawn(async move {
            if let Some(db) = db {
                if let Err(e) = db.create_note_with_id(&optimistic_note).await {
                    tracing::error!("Failed to persist note: {}", e);
                    // Note: Don't rollback - user can continue editing
                } else {
                    // Invalidate query to sync state
                    invalidate_notes_query().await;
                }
            }
        });
    };

    let delete_note = move |_| {
        let note_id = *state.current_note_id.read();
        if let Some(id) = note_id {
            // Update UI immediately (optimistic)
            state.notes.write().retain(|n| n.id != id);
            state.current_note_id.set(None);

            tracing::info!("Deleted note (optimistic): {}", id);

            // Persist in background
            let db = state.db_service.read().clone();
            spawn(async move {
                if let Some(db) = db {
                    if let Err(e) = db.delete_note(&id).await {
                        tracing::error!("Failed to persist delete: {}", e);
                    } else {
                        // Invalidate query to sync state
                        invalidate_notes_query().await;
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
