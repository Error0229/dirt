//! Shared note actions used by UI components.

use dioxus::prelude::*;
use dirt_core::NoteId;

use crate::queries::invalidate_notes_query;
use crate::state::AppState;

/// Create a new empty note and select it once the store has saved it.
///
/// The store assigns the note's ID, so the note joins the list only after
/// the write returns.
pub fn create_note(state: &AppState) {
    let db = state.db_service.read().clone();
    let mut state = *state;
    spawn(async move {
        if let Some(db) = db {
            match db.create_note("").await {
                Ok(note) => {
                    tracing::info!("Created new note: {}", note.id);
                    let note_id = note.id;
                    state.notes.write().insert(0, note);
                    state.current_note_id.set(Some(note_id));
                    invalidate_notes_query().await;
                }
                Err(e) => {
                    tracing::error!("Failed to persist note: {}", e);
                }
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

    tracing::info!("Deleted note (optimistic): {}", note_id);

    let db = state.db_service.read().clone();
    spawn(async move {
        if let Some(db) = db {
            if let Err(e) = db.delete_note(&note_id).await {
                tracing::error!("Failed to persist delete: {}", e);
            }
            // Always re-sync the query: on success to confirm, on failure to rollback the optimistic removal.
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
