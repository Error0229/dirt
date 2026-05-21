//! Mobile shell state types and context object.
//!
//! `SyncStatus` and `View` are pure data and stay outside any cfg guard
//! so the sync-worker tests on a non-android host can construct them
//! freely. The `AppState` context object pulls in dioxus signals and is
//! therefore android-only.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::sync::Arc;

#[cfg(target_os = "android")]
use dioxus::prelude::Signal;
#[cfg(target_os = "android")]
use dirt_core::auth::StoredToken;
use dirt_core::auth::{AuthClient, TokenStore};
#[cfg(target_os = "android")]
use dirt_core::models::{Note, NoteId};
#[cfg(target_os = "android")]
use dirt_core::sync::session_client::SessionApiClient;
#[cfg(target_os = "android")]
use tokio::sync::mpsc::UnboundedSender;

#[cfg(target_os = "android")]
use crate::data::MobileNoteStore;
#[cfg(target_os = "android")]
use crate::services::{SyncEvent, SyncWorkerHandle};

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
    /// Settings panel (Account row in P2.7; more tabs may follow).
    Settings,
}

/// Auth dependencies that live for the lifetime of the process.
///
/// Same shape as `dirt-desktop`'s `AuthDeps`: `Clone` (inner handles are
/// all `Arc`-shaped) so the type threads cheaply through Dioxus
/// context. Held in its own context (`Signal<AuthDeps>`) so the Account
/// row reads it without depending on the dozen unrelated note-side
/// signals on `AppState`.
#[derive(Clone)]
pub struct AuthDeps {
    /// HTTP client for `/v1/auth/*`. `None` when no API base URL is
    /// available — the Account row surfaces a config error rather than
    /// silently failing.
    pub auth_client: Option<Arc<AuthClient>>,
    /// Persistent token store. Always present — on Android this is the
    /// JNI-backed `EncryptedPrefsTokenStore`, on the host (tests) it's
    /// the file-based `FileTokenStore`. Construction does not touch the
    /// platform store until a load / save / clear actually fires.
    pub token_store: Arc<dyn TokenStore>,
    /// Normalized API base URL used to (re)build `SessionApiClient`
    /// after login. `None` mirrors `auth_client.is_none()`.
    pub api_base_url: Option<String>,
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
    /// Currently signed-in user, if any. Source of truth for "is the
    /// user signed in"; the Account row in Settings reads this.
    pub signed_in: Signal<Option<StoredToken>>,
    /// Refreshing session API client. Present iff `signed_in` is
    /// present; the sync worker holds its own `Arc<SessionApiClient>`
    /// so swapping this signal doesn't kill the running worker.
    pub session_client: Signal<Option<Arc<SessionApiClient>>>,
    /// Long-lived [`SyncEvent`] sender shared by every worker spawn
    /// (startup-hydrate, post-login). Owned by `AppShell` along with
    /// the corresponding drainer task — both live in the root scope,
    /// so the bridge from worker → UI signals stays alive when the
    /// user navigates between List / Editor / Settings.
    ///
    /// A previous version spawned the drainer inside
    /// `spawn_session_worker`, but Dioxus' `spawn` attaches the task
    /// to the *calling* component's scope. When the Settings view
    /// re-spawned the worker after sign-in and the user navigated
    /// back to the list, the drainer was cancelled — leaving the
    /// worker alive but its status events silently dropped.
    pub events_tx: Signal<Option<UnboundedSender<SyncEvent>>>,
}

#[cfg(target_os = "android")]
impl AppState {
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
