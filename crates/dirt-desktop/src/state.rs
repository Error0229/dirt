//! Application state management
//!
//! Global state accessible via Dioxus context providers.

use std::sync::Arc;

use dioxus::prelude::*;

use dirt_core::auth::{AuthClient, StoredToken, TokenStore};
use dirt_core::models::{Note, NoteId, Settings};
pub use dirt_core::state::SyncState as SyncStatus;
use dirt_core::sync::session_client::SessionApiClient;

use crate::services::{DatabaseService, SyncWorkerHandle, TranscriptionService};
use crate::theme::ResolvedTheme;

/// Auth dependencies that live for the lifetime of the process.
///
/// `Clone` is intentional — the inner handles are all `Arc`-shaped, so
/// cloning is cheap and the type is `Copy`-friendly to thread through
/// Dioxus context. Pulled into its own struct (rather than dumped flat
/// into `AppState`) so the auth tab can take it without depending on
/// the dozen unrelated note-side signals.
#[derive(Clone)]
pub struct AuthDeps {
    /// HTTP client for `/v1/auth/*`. `None` when `DIRT_API_BASE_URL`
    /// is missing/invalid — the login UI surfaces a config error
    /// instead of silently failing.
    pub auth_client: Option<Arc<AuthClient>>,
    /// Persistent token store. Always present (the keyring backend
    /// doesn't touch the OS until a load/save/clear actually fires).
    pub token_store: Arc<dyn TokenStore>,
    /// Normalized API base URL used to (re)build `SessionApiClient`
    /// after login. `None` mirrors `auth_client.is_none()`.
    pub api_base_url: Option<String>,
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
    /// Optional transcription service.
    pub transcription_service: Signal<Option<Arc<TranscriptionService>>>,
    /// Currently signed-in user, if any. `Some(stored)` is the source
    /// of truth for "is the user signed in" — UI surfaces (settings
    /// account row, future toolbar) read this signal.
    pub signed_in: Signal<Option<StoredToken>>,
    /// Refreshing session API client. Present iff `signed_in` is
    /// present; the sync worker holds its own `Arc<SessionApiClient>`
    /// so swapping this signal doesn't kill the running worker.
    pub session_client: Signal<Option<Arc<SessionApiClient>>>,
    /// Sync worker handle. `None` means no sync is running — either
    /// the user is signed out, or the worker was shut down after a
    /// failed configuration.
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
    /// quickly. No-op when the worker isn't running (signed out,
    /// misconfigured env). Mutation sites call this after a successful
    /// DB write so the worker debounces local edits into a single
    /// sync cycle.
    pub fn trigger_sync(&self) {
        if let Some(handle) = (self.sync_worker)().as_ref() {
            handle.trigger();
        }
    }
}
