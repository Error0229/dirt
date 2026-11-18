//! Application state management
//!
//! Global state accessible via Dioxus context providers.

use std::sync::Arc;

use dioxus::prelude::*;

use dirt_core::models::{NoteId, Settings};
use dirt_core::session::{Session, SessionState};
use dirt_core::sync::worker::{SyncStatus, Trigger};
use dirt_core::Store;

use crate::theme::ResolvedTheme;

/// Where Settings opens: the tab it shows first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    Data,
    Sync,
    Account,
    Devices,
}

/// Global application state
#[derive(Clone, Copy)]
pub struct AppState {
    /// The note open on the page, if one is. The main screen sets it; to
    /// open a note from elsewhere, ask with [`Self::open_request`].
    pub current_note_id: Signal<Option<NoteId>>,
    /// A note to open on the page, asked for from outside the main
    /// screen: Settings → Sync opens a note that can't sync.
    pub open_request: Signal<Option<NoteId>>,
    /// Raised whenever the notes in the store may have changed: written
    /// here, brought by a sync, or another store taken into use. Whatever
    /// shows notes reads them again.
    pub notes_read: Signal<u64>,
    /// Raised when a sync has brought notes from another device, so that
    /// the list can show which arrived.
    pub pulled: Signal<u64>,
    /// Application settings
    pub settings: Signal<Settings>,
    /// Resolved theme (light/dark based on settings and system preference),
    /// for the old settings panel and quick capture until they are
    /// replaced.
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
    /// The tab Settings shows when it opens.
    pub settings_tab: Signal<SettingsTab>,
    /// Whether quick capture overlay is active
    pub quick_capture_open: Signal<bool>,
    /// Quit was chosen: the main screen hands over what its page holds,
    /// and the app waits a moment for the store's queue before it ends.
    pub quitting: Signal<bool>,
}

impl AppState {
    /// Tells the sync worker that a note was created, edited or deleted, so
    /// that it is sent once changes stop arriving, and has what shows notes
    /// read them again.
    pub fn note_changed(&self) {
        if let Some(session) = self.session.peek().as_ref() {
            session.notify(Trigger::Changed);
        }
        self.reread();
    }

    /// Has whatever shows notes read them again.
    pub fn reread(&self) {
        let mut read = self.notes_read;
        *read.write() += 1;
    }

    /// Opens Settings at `tab`.
    pub fn open_settings(&self, tab: SettingsTab) {
        let (mut open, mut at) = (self.settings_open, self.settings_tab);
        at.set(tab);
        open.set(true);
    }

    /// Saves this device's settings, as they are now in [`Self::settings`].
    pub fn save_settings(&self) {
        let settings = self.settings.peek().clone();
        let store = self.device_store.peek().clone();
        spawn(async move {
            if let Some(store) = store {
                if let Err(error) = store.save_settings(&settings).await {
                    tracing::error!("Failed to save settings: {error}");
                }
            }
        });
    }
}
