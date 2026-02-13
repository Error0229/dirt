//! Application state management
//!
//! Global state accessible via Dioxus context providers.

use std::sync::Arc;

use dioxus::prelude::*;

use dirt_core::models::{Note, NoteId, Settings};

use crate::services::{
    AuthSession, DatabaseService, MediaApiClient, SupabaseAuthService, TranscriptionService,
};
use crate::theme::ResolvedTheme;

/// Current sync status for the app
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    Synced,
    Syncing,
    Offline,
    Error,
}

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
    /// Auth service if cloud auth is configured
    pub auth_service: Signal<Option<Arc<SupabaseAuthService>>>,
    /// Managed media API client, if configured
    pub media_api_client: Signal<Option<Arc<MediaApiClient>>>,
    /// Optional transcription service.
    pub transcription_service: Signal<Option<Arc<TranscriptionService>>>,
    /// Active auth session, if signed in
    pub auth_session: Signal<Option<AuthSession>>,
    /// Last auth initialization/sign-in error for UI display
    pub auth_error: Signal<Option<String>>,
    /// Monotonic reconnect trigger for db reinitialization flows.
    pub db_reconnect_version: Signal<u64>,
    /// Current sync status
    pub sync_status: Signal<SyncStatus>,
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
}

impl AppState {
    /// Get the currently selected note
    #[must_use]
    pub fn current_note(&self) -> Option<Note> {
        let current_id = (self.current_note_id)();
        current_id.and_then(|id| (self.notes)().into_iter().find(|note| note.id == id))
    }

    /// Get filtered notes based on search query and tag filter
    #[must_use]
    pub fn filtered_notes(&self) -> Vec<Note> {
        let notes = (self.notes)();
        let query = (self.search_query)().to_lowercase();
        let tag_filter = (self.active_tag_filter)();

        notes
            .into_iter()
            .filter(|note| !note.is_deleted)
            .filter(|note| {
                if query.is_empty() {
                    true
                } else {
                    note.content.to_lowercase().contains(&query)
                }
            })
            .filter(|note| {
                tag_filter
                    .as_ref()
                    .map_or(true, |tag| note.tags().iter().any(|t| t == tag))
            })
            .collect()
    }

    /// Track a pending change for a note until the next successful sync.
    pub fn enqueue_pending_change(&mut self, note_id: NoteId) {
        let mut pending_notes = self.pending_sync_note_ids.write();
        if !pending_notes.contains(&note_id) {
            pending_notes.push(note_id);
            self.pending_sync_count.set(pending_notes.len());
        }
    }
}
