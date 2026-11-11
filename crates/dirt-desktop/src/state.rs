//! Application state management
//!
//! Global state accessible via Dioxus context providers.

use std::sync::Arc;

use dioxus::prelude::*;

use dirt_core::auth::{AuthClient, StoredToken, TokenStore};
use dirt_core::models::{Note, NoteId, Settings};
use dirt_core::Store;

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
    /// Normalized API base URL, shown on the Account tab. `None`
    /// mirrors `auth_client.is_none()`.
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
    /// The local note store (wrapped in Arc for sharing)
    pub db_service: Signal<Option<Arc<Store>>>,
    /// Why the note store could not be opened, if it could not. The app
    /// has no notes to show in that case, and must say why.
    pub startup_error: Signal<Option<String>>,
    /// Currently signed-in user, if any. `Some(stored)` is the source
    /// of truth for "is the user signed in" — UI surfaces (settings
    /// account row, future toolbar) read this signal.
    pub signed_in: Signal<Option<StoredToken>>,
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
}
