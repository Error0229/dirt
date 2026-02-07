//! Shared note actions used by UI components.

use dioxus::prelude::*;
use dirt_core::models::Note;

use crate::queries::invalidate_notes_query;
use crate::state::AppState;

/// Create a new note with optimistic UI update and background persistence.
pub fn create_note_optimistic(state: &mut AppState) {
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
}
