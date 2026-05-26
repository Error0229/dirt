//! Shared note actions used by UI components.

use dioxus::prelude::*;
use dirt_core::models::Note;
use dirt_core::NoteId;

use crate::queries::invalidate_notes_query;
use crate::state::AppState;

/// Create a new note with optimistic UI update and background persistence.
pub fn create_note_optimistic(state: &mut AppState) {
    // Resolve the DB up-front so the optimistic note carries the
    // active user's id from the moment it appears in the UI. Without
    // a DB ready there is nowhere to persist anyway — bail cleanly
    // rather than creating an orphan optimistic row.
    let db = state.db_service.read().clone();
    let Some(db) = db else {
        tracing::warn!("Skipping optimistic note creation — database is not ready yet");
        return;
    };

    let optimistic_note = match Note::new_for_user("", db.user_id()) {
        Ok(note) => note,
        Err(err) => {
            tracing::error!("Failed to build optimistic note: {err}");
            return;
        }
    };
    let note_id = optimistic_note.id;

    // Update UI immediately (optimistic)
    state.notes.write().insert(0, optimistic_note.clone());
    state.current_note_id.set(Some(note_id));
    state.enqueue_pending_change(note_id);

    tracing::info!("Created new note (optimistic): {}", note_id);

    // Persist in background
    let worker = state.sync_worker.read().clone();
    spawn(async move {
        if let Err(e) = db.create_note_with_id(&optimistic_note).await {
            tracing::error!("Failed to persist note: {}", e);
        } else {
            invalidate_notes_query().await;
            if let Some(worker) = worker {
                worker.trigger();
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
    // Remove from pending sync queue — the note no longer exists locally,
    // so it should not be treated as a pending edit.
    {
        let mut pending = state.pending_sync_note_ids.write();
        pending.retain(|id| *id != note_id);
        state.pending_sync_count.set(pending.len());
    }

    tracing::info!("Deleted note (optimistic): {}", note_id);

    let db = state.db_service.read().clone();
    let worker = state.sync_worker.read().clone();
    spawn(async move {
        if let Some(db) = db {
            if let Err(e) = db.delete_note(&note_id).await {
                tracing::error!("Failed to persist delete: {}", e);
            } else if let Some(worker) = worker {
                // Push the tombstone now so other devices see the
                // delete promptly. Sync trigger only fires on success;
                // a failed delete has nothing to push.
                worker.trigger();
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
