//! Application state management
//!
//! Global state accessible via Dioxus context providers.

use std::sync::Arc;

use dioxus::prelude::*;

use dirt_core::models::{Note, NoteId, Settings};
use dirt_core::session::{Session, SessionState};
use dirt_core::sync::worker::{SyncStatus, Trigger};
use dirt_core::Store;

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
    /// How many notes the list shows, and how many a search may find. It
    /// grows when the person asks for older notes.
    pub listed_notes: Signal<usize>,
    /// Application settings
    pub settings: Signal<Settings>,
    /// Resolved theme (light/dark based on settings and system preference)
    pub theme: Signal<ResolvedTheme>,
    /// The note store in use: the signed-in account's, or the local one.
    /// It is replaced when the person signs in or out.
    pub db_service: Signal<Option<Arc<Store>>>,
    /// The store that holds this device's preferences, whoever is signed
    /// in.
    pub device_store: Signal<Option<Arc<Store>>>,
    /// Why the note store could not be opened, if it could not. The app
    /// has no notes to show in that case, and must say why.
    pub startup_error: Signal<Option<String>>,
    /// The session, when this build was given a server to sync with.
    pub session: Signal<Option<Session>>,
    /// Who is signed in, for the interface to render.
    pub session_state: Signal<SessionState>,
    /// How sync is going, for the interface to render.
    pub sync_status: Signal<SyncStatus>,
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

    /// Tells the sync worker that a note was created, edited or deleted, so
    /// that it is sent once changes stop arriving.
    pub fn note_changed(&self) {
        if let Some(session) = self.session.peek().as_ref() {
            session.notify(Trigger::Changed);
        }
    }
}
