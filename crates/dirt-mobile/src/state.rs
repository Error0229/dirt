//! Mobile shell state types and context object.

use std::sync::Arc;

use dioxus::prelude::*;
use dirt_core::models::{NoteId, Settings};
use dirt_core::session::{Session, SessionState};
use dirt_core::sync::worker::{SyncStatus, Trigger};
use dirt_core::Store;
use dirt_ui::settings::SettingsNav;

use crate::data::MobileNoteStore;

/// Context object passed via `use_context_provider` to every component.
///
/// Each field is a dioxus `Signal`, so cloning the struct is cheap and
/// downstream components can mutate state without prop drilling.
#[derive(Clone, Copy)]
pub struct AppState {
    /// Whether Settings is open over the main screen, and where.
    pub nav: SettingsNav,
    /// A failure that the main screen shows: the local database could not
    /// be opened, the device's secure storage could not be used, or what
    /// was shared could not be saved.
    pub startup_issue: Signal<Option<String>>,
    /// The note store in use: the signed-in account's, or the local one.
    /// It is replaced when the person signs in or out.
    pub store: Signal<Option<Arc<MobileNoteStore>>>,
    /// The store that holds this device's settings, whoever is signed in.
    pub device_store: Signal<Option<Store>>,
    /// This device's settings: the theme, the accent and the text size.
    pub settings: Signal<Settings>,
    /// The session, when this build was given a server to sync with and
    /// the device's secure storage works.
    pub session: Signal<Option<Session>>,
    /// Who is signed in, for the interface to render.
    pub session_state: Signal<SessionState>,
    /// How sync is going, for the interface to render.
    pub sync_status: Signal<SyncStatus>,
    /// Raised whenever the notes in the store may have changed: written
    /// here, brought by a sync, or another store taken into use. Whatever
    /// shows notes reads them again.
    pub notes_read: Signal<u64>,
    /// Raised when a sync has brought notes from another device, so that
    /// the list can show which arrived.
    pub pulled: Signal<u64>,
    /// The note open on the page, if one is. The main screen sets it.
    pub current_note_id: Signal<Option<NoteId>>,
    /// A note to open, asked for from outside the main screen: Settings →
    /// Sync opens a note that can't sync.
    pub open_request: Signal<Option<NoteId>>,
    /// Raised when the activity may hold something from outside the app:
    /// a share, or the widget asking for a page. The main screen asks.
    pub capture_ready: Signal<u64>,
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
