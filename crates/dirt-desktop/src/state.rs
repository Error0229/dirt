//! Application state management
//!
//! Global state accessible via Dioxus context providers.

use std::sync::Arc;

use dioxus::prelude::*;

use dirt_core::models::{Note, NoteId, Settings};
pub use dirt_core::state::SyncState as SyncStatus;

use crate::services::{DatabaseService, SyncWorkerHandle, TranscriptionService};
use crate::theme::ResolvedTheme;

/// Global application state
#[derive(Clone, Copy)]
pub struct AppState {
    /// All notes loaded in the app
    pub notes: Signal<Vec<Note>>,
    /// Currently selected note ID
    pub current_note_id: Signal<Option<NoteId>>,
    /// Current search query
    pub search_query: Signal<String>,
    /// Active tag filter
    pub active_tag_filter: Signal<Option<String>>,
    /// Application settings
    pub settings: Signal<Settings>,
    /// Resolved theme (light/dark based on settings and system preference)
    pub theme: Signal<ResolvedTheme>,
    /// Database service (wrapped in Arc for sharing)
    pub db_service: Signal<Option<Arc<DatabaseService>>>,
    /// Optional transcription service.
    pub transcription_service: Signal<Option<Arc<TranscriptionService>>>,
    /// Sync worker handle. `None` means sync is misconfigured and the
    /// worker isn't running — the UI shows `sync_issue` so the user
    /// notices instead of getting silent staleness.
    pub sync_worker: Signal<Option<SyncWorkerHandle>>,
    /// Current sync status
    pub sync_status: Signal<SyncStatus>,
    /// Last sync subsystem error shown in settings diagnostics
    pub sync_issue: Signal<Option<String>>,
    /// Timestamp (unix ms) of the most recent successful sync
    pub last_sync_at: Signal<Option<i64>>,
    /// Count of local changes pending cloud sync
    pub pending_sync_count: Signal<usize>,
    /// Unique note IDs currently represented in pending changes
    pub pending_sync_note_ids: Signal<Vec<NoteId>>,
    /// Whether settings panel is open
    pub settings_open: Signal<bool>,
    /// Whether quick capture overlay is active
    pub quick_capture_open: Signal<bool>,
    /// Whether note list panel is visible
    pub note_list_visible: Signal<bool>,
}

impl AppState {
    /// Get the currently selected note
    #[must_use]
    pub fn current_note(&self) -> Option<Note> {
        let current_id = (self.current_note_id)();
        current_id.and_then(|id| (self.notes)().into_iter().find(|note| note.id == id))
    }

    /// Track a pending change for a note until the next successful sync.
    pub fn enqueue_pending_change(&mut self, note_id: NoteId) {
        let mut pending_notes = self.pending_sync_note_ids.write();
        if !pending_notes.contains(&note_id) {
            pending_notes.push(note_id);
            self.pending_sync_count.set(pending_notes.len());
        }
    }

    /// Kick the sync worker so a pending mutation reaches the server
    /// quickly. No-op when the worker isn't running (misconfigured
    /// env). Mutation sites call this after a successful DB write so
    /// the worker debounces local edits into a single sync cycle.
    pub fn trigger_sync(&self) {
        if let Some(handle) = (self.sync_worker)().as_ref() {
            handle.trigger();
        }
    }
}
