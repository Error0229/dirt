//! Shared note actions used by UI components.

use dioxus::prelude::*;
use dirt_core::models::Note;
use dirt_core::NoteId;

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
    state.enqueue_pending_change(note_id);

    tracing::info!("Created new note (optimistic): {}", note_id);

    // Persist in background
    let db = state.db_service.read().clone();
    spawn(async move {
        if let Some(db) = db {
            if let Err(e) = db.create_note_with_id(&optimistic_note).await {
                tracing::error!("Failed to persist note: {}", e);
            } else {
                invalidate_notes_query().await;
            }
        }
    });
}

/// Delete a note with optimistic UI removal and background persistence.
pub fn delete_note_optimistic(state: &mut AppState, note_id: NoteId) {
    state.notes.write().retain(|n| n.id != note_id);
    if (state.current_note_id)() == Some(note_id) {
        state.current_note_id.set(None);
    }
    state.enqueue_pending_change(note_id);

    tracing::info!("Deleted note (optimistic): {}", note_id);

    let db = state.db_service.read().clone();
    spawn(async move {
        if let Some(db) = db {
            if let Err(e) = db.delete_note(&note_id).await {
                tracing::error!("Failed to persist delete: {}", e);
            }
            // Always re-sync: on success to confirm, on failure to rollback the optimistic removal
            invalidate_notes_query().await;
        }
    });
}

/// Optimistically update a note's content in the local notes list.
pub fn update_note_content(state: &mut AppState, note_id: NoteId, new_content: String) {
    let mut notes = state.notes.write();
    if let Some(note) = notes.iter_mut().find(|note| note.id == note_id) {
        note.content = new_content;
        note.updated_at = chrono::Utc::now().timestamp_millis();
    }
}
