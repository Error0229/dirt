//! Application state management
//!
//! Global state accessible via Dioxus context providers.

use dioxus::prelude::*;

use dirt_core::models::{Note, NoteId};

use crate::services::DatabaseService;
use crate::theme::Theme;

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
    /// Current theme
    #[allow(dead_code)] // Will be used when theme toggle UI is implemented
    pub theme: Signal<Theme>,
    /// Database service
    pub db_service: Signal<Option<DatabaseService>>,
    /// Whether quick capture overlay is visible
    #[allow(dead_code)] // Accessed via context, not directly
    pub show_quick_capture: Signal<bool>,
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
}
