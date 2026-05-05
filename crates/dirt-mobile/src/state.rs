//! Mobile shell state types and context object.
//!
//! `SyncStatus` and `View` are pure data and stay outside any cfg guard
//! so the sync-worker tests on a non-android host can construct them
//! freely. The `AppState` context object pulls in dioxus signals and is
//! therefore android-only.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

#[cfg(target_os = "android")]
use std::sync::Arc;

#[cfg(target_os = "android")]
use dioxus::prelude::Signal;
#[cfg(target_os = "android")]
use dirt_core::models::{Note, NoteId};

#[cfg(target_os = "android")]
use crate::data::MobileNoteStore;
#[cfg(target_os = "android")]
use crate::services::SyncWorkerHandle;

/// Coarse sync status surfaced to the UI by the worker.
///
/// Re-exports the cross-platform `dirt_core::state::SyncState` so the
/// mobile shell speaks the same vocabulary as desktop and any future
/// shared UI helpers.
pub use dirt_core::state::SyncState as SyncStatus;

/// Short label suitable for an indicator chip.
#[must_use]
pub const fn sync_status_label(status: SyncStatus) -> &'static str {
    match status {
        SyncStatus::Offline => "Offline",
        SyncStatus::Syncing => "Syncing",
        SyncStatus::Synced => "Synced",
        SyncStatus::Error => "Error",
    }
}

/// Top-level navigation state for the shell.
#[cfg(target_os = "android")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Note list is the home screen.
    List,
    /// Editor is open for a specific note, or for a new draft when
    /// `selected_note_id` is `None`.
    Editor,
}

/// Context object passed via `use_context_provider` to every component.
///
/// Each field is a dioxus `Signal`, so cloning the struct is cheap and
/// downstream components can mutate state without prop drilling.
#[cfg(target_os = "android")]
#[derive(Clone, Copy)]
pub struct AppState {
    pub notes: Signal<Vec<Note>>,
    pub selected_note_id: Signal<Option<NoteId>>,
    pub view: Signal<View>,
    pub sync_status: Signal<SyncStatus>,
    pub sync_issue: Signal<Option<String>>,
    pub last_sync_at: Signal<Option<i64>>,
    pub store: Signal<Option<Arc<MobileNoteStore>>>,
    pub sync_worker: Signal<Option<SyncWorkerHandle>>,
}

#[cfg(target_os = "android")]
impl AppState {
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
