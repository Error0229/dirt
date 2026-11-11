//! Mobile shell state types and context object.
//!
//! `AuthDeps` has no Dioxus dependency and stays outside any cfg guard
//! so the sign-in flow tests on a non-android host can construct it
//! freely. `View` and the `AppState` context object are only used by
//! the Dioxus components and are therefore android-only.
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
use crate::data::MobileNoteStore;

/// What the interface says wherever it would otherwise report on sync.
#[cfg(target_os = "android")]
pub const SYNC_UNAVAILABLE_MESSAGE: &str =
    "Sync is unavailable while Dirt's sync is being rebuilt. \
     Your notes are stored on this device only.";

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
    /// A failure from startup that the list view shows: the local
    /// database could not be opened, or the saved session could not be
    /// read.
    pub startup_issue: Signal<Option<String>>,
    pub store: Signal<Option<Arc<MobileNoteStore>>>,
    /// Currently signed-in user, if any. Source of truth for "is the
    /// user signed in"; the Account row in Settings reads this.
    pub signed_in: Signal<Option<StoredToken>>,
}
