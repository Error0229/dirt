//! Mobile shell state types and context object.

use std::sync::Arc;

use dioxus::prelude::{ReadableExt, Signal};
use dirt_core::models::{Note, NoteId};
use dirt_core::session::{Session, SessionState};
use dirt_core::sync::worker::{SyncStatus, Trigger};

use crate::data::MobileNoteStore;

/// Top-level navigation state for the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Note list is the home screen.
    List,
    /// Editor is open for a specific note, or for a new draft when
    /// `selected_note_id` is `None`.
    Editor,
    /// Settings: the account and sync.
    Settings,
    /// The account's devices: letting one in, the recovery phrase.
    Devices,
}

/// Context object passed via `use_context_provider` to every component.
///
/// Each field is a dioxus `Signal`, so cloning the struct is cheap and
/// downstream components can mutate state without prop drilling.
#[derive(Clone, Copy)]
pub struct AppState {
    pub notes: Signal<Vec<Note>>,
    pub selected_note_id: Signal<Option<NoteId>>,
    pub view: Signal<View>,
    /// What is typed in the list's search box.
    pub search_query: Signal<String>,
    /// The tag the list is narrowed to, if any.
    pub tag_filter: Signal<Option<String>>,
    /// How many notes the list shows, and how many a search may find. It
    /// grows when the person asks for more.
    pub listed_notes: Signal<usize>,
    /// A failure from startup that the list view shows: the local
    /// database could not be opened, or the device's secure storage could
    /// not be used.
    pub startup_issue: Signal<Option<String>>,
    /// The note store in use: the signed-in account's, or the local one.
    /// It is replaced when the person signs in or out.
    pub store: Signal<Option<Arc<MobileNoteStore>>>,
    /// The session, when this build was given a server to sync with and
    /// the device's secure storage works.
    pub session: Signal<Option<Session>>,
    /// Who is signed in, for the interface to render.
    pub session_state: Signal<SessionState>,
    /// How sync is going, for the interface to render.
    pub sync_status: Signal<SyncStatus>,
}

impl AppState {
    /// Tells the sync worker that a note was created, edited or deleted.
    pub fn note_changed(&self) {
        if let Some(session) = self.session.peek().as_ref() {
            session.notify(Trigger::Changed);
        }
    }
}
