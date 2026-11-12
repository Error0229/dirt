//! Mobile app shell.
//!
//! Owns start-up: it opens the session, which says who is signed in and
//! which note store is in use, and keeps the interface following it. A build
//! with no server, or a device whose secure storage cannot be used, has no
//! session and keeps its notes in the local store.
//!
//! Signing in leaves the app for the system browser. Android brings the
//! person back through the custom address `dev.dirt.app://callback`, which
//! `MainActivity` writes to a file for this code to find. Android may stop
//! the app while the browser is in front, so the redirect is also looked
//! for at start-up; the session saved the attempt before the browser
//! opened.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::api::HttpTransport;
use dirt_core::secrets::slot_name;
use dirt_core::session::{Session, SessionConfig, SessionError, SessionState};
use dirt_core::store::{store_path, StoreName};
use dirt_core::sync::worker::{SyncStatus, Timing, Trigger};
use dirt_core::Store;

use crate::bootstrap_config::load_bootstrap_config;
use crate::config::{android_files_directory, default_mobile_data_directory};
use crate::data::MobileNoteStore;
use crate::services::secure_storage::EncryptedPrefs;
use crate::state::{AppState, View};
use crate::ui::MOBILE_UI_STYLES;
use crate::views::{Editor, List, Settings};

/// Where the sign-in service sends the browser afterwards. The manifest
/// registers the app for this address.
pub const REDIRECT_URI: &str = "dev.dirt.app://callback";

/// The file `MainActivity` writes the redirect address to.
const REDIRECT_FILE: &str = "redirect.txt";

/// The name other devices see for this one.
const DEVICE_NAME: &str = "Android device";

fn open_session(server_url: &str) -> Result<Session, String> {
    let secrets =
        EncryptedPrefs::open(&slot_name(server_url)).map_err(|error| error.to_string())?;
    let transport = HttpTransport::new(server_url).map_err(|error| error.to_string())?;
    Session::open(
        SessionConfig {
            data_folder: default_mobile_data_directory(),
            device_name: DEVICE_NAME.to_owned(),
            background_sync: true,
            // Android may stop the app while the browser is in front.
            persist_attempts: true,
            timing: Timing::default(),
        },
        Arc::new(transport),
        Arc::new(secrets),
    )
    .map_err(|error| error.to_string())
}

fn open_local_store() -> Result<Store, String> {
    Store::open(store_path(
        &default_mobile_data_directory(),
        &StoreName::Local,
    ))
    .map_err(|error| error.to_string())
}

fn redirect_file() -> Option<PathBuf> {
    android_files_directory().map(|folder| folder.join(REDIRECT_FILE))
}

/// The redirect address `MainActivity` left, if any. Taking it removes it.
fn take_redirect() -> Option<String> {
    let path = redirect_file()?;
    let address = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    Some(address)
}

/// Finishes a sign-in with a redirect, and says what to tell the person if
/// it failed. A redirect with no attempt to match is one that was already
/// used or was cancelled, and is ignored.
async fn finish_sign_in(session: &Session, redirect: &str) -> Option<String> {
    match session.finish_sign_in(redirect).await {
        Ok(_) | Err(SessionError::NoAttempt) => None,
        Err(error) => Some(format!("Sign-in failed: {error}")),
    }
}

#[component]
pub fn AppShell() -> Element {
    let bootstrap = load_bootstrap_config();
    let server_url = bootstrap.managed_api_base_url();

    let mut notes = use_signal(Vec::new);
    let mut selected_note_id = use_signal(|| None);
    let mut view = use_signal(|| View::List);
    let mut startup_issue = use_signal(|| None::<String>);
    let mut store: Signal<Option<Arc<MobileNoteStore>>> = use_signal(|| None);
    let mut session: Signal<Option<Session>> = use_signal(|| None);
    let mut session_state = use_signal(|| SessionState::SignedOut);
    let mut sync_status = use_signal(SyncStatus::default);

    use_context_provider(|| AppState {
        notes,
        selected_note_id,
        view,
        startup_issue,
        store,
        session,
        session_state,
        sync_status,
    });

    // One-shot start-up. `use_resource` only runs again if a signal it
    // read changes, and it reads none.
    let _init = use_resource(move || {
        let server_url = server_url.clone();
        async move {
            let opened_session = match server_url.as_deref().map(open_session) {
                Some(Ok(opened)) => Some(opened),
                Some(Err(error)) => {
                    tracing::error!("The session could not be opened: {error}");
                    startup_issue.set(Some(format!(
                        "Sign-in is unavailable on this device: {error}"
                    )));
                    None
                }
                None => None,
            };
            let note_store = opened_session
                .as_ref()
                .map_or_else(open_local_store, |opened| Ok(opened.store()));
            let note_store = match note_store {
                Ok(note_store) => Arc::new(MobileNoteStore::new(note_store)),
                Err(error) => {
                    tracing::error!("Failed to open mobile DB: {error}");
                    startup_issue.set(Some(format!("Failed to open local database: {error}")));
                    return;
                }
            };
            match note_store.list_notes().await {
                Ok(initial) => notes.set(initial),
                Err(error) => tracing::warn!("Initial note listing failed: {error}"),
            }
            store.set(Some(note_store));

            if let Some(opened) = &opened_session {
                session_state.set(opened.state());
                sync_status.set(opened.sync_status());
            }
            session.set(opened_session.clone());

            // The app may have been stopped while the browser was open and
            // started again by the redirect.
            if let (Some(opened), Some(redirect)) = (opened_session, take_redirect()) {
                if let Some(problem) = finish_sign_in(&opened, &redirect).await {
                    startup_issue.set(Some(problem));
                }
            }
        }
    });

    // Follow the session: when the person signs in or out, the store in use
    // changes with them.
    let _session_watch = use_resource(move || {
        let current = session();
        async move {
            let Some(current) = current else { return };
            let mut states = current.watch_state();
            while states.changed().await.is_ok() {
                let state = *states.borrow_and_update();
                session_state.set(state);
                let next = current.store();
                let same_store = store
                    .peek()
                    .as_ref()
                    .is_some_and(|open| open.path() == next.path());
                if !same_store {
                    let next = Arc::new(MobileNoteStore::new(next));
                    selected_note_id.set(None);
                    if *view.peek() == View::Editor {
                        view.set(View::List);
                    }
                    notes.set(next.list_notes().await.unwrap_or_default());
                    store.set(Some(next));
                }
                if state == SessionState::SignedOut {
                    current.remove_signed_out_stores();
                }
            }
        }
    });

    // Follow sync: show its status, and reload the list when notes arrive
    // from another device.
    let _sync_watch = use_resource(move || {
        let current = session();
        async move {
            let Some(current) = current else { return };
            let mut statuses = current.watch_sync();
            let mut pulled = statuses.borrow().pulled;
            while statuses.changed().await.is_ok() {
                let status = statuses.borrow_and_update().clone();
                if status.pulled != pulled {
                    pulled = status.pulled;
                    let open = store.peek().clone();
                    if let Some(open) = open {
                        if let Ok(latest) = open.list_notes().await {
                            notes.set(latest);
                        }
                    }
                }
                sync_status.set(status);
            }
        }
    });

    // While a sign-in is under way, watch for the redirect the browser
    // sends the person back with.
    let _redirect_watch = use_resource(move || {
        let state = session_state();
        let current = session();
        async move {
            let Some(current) = current else { return };
            if state != SessionState::SigningIn {
                return;
            }
            loop {
                if let Some(redirect) = take_redirect() {
                    if let Some(problem) = finish_sign_in(&current, &redirect).await {
                        startup_issue.set(Some(problem));
                    }
                    return;
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    });

    // Coming back to the app is a reason to sync: notes may have been
    // written elsewhere while it was in the background. The web view says
    // when that happens, because its page becomes visible again. The worker
    // skips the sync if one ran a moment ago.
    use_future(move || async move {
        let mut became_visible = document::eval(
            r#"
            document.addEventListener("visibilitychange", () => {
                if (document.visibilityState === "visible") {
                    dioxus.send(true);
                }
            });
            // Stay alive, so that the messages above keep arriving.
            await new Promise(() => {});
            "#,
        );
        while became_visible.recv::<bool>().await.is_ok() {
            if let Some(current) = session.peek().clone() {
                current.notify(Trigger::Returned);
            }
        }
    });

    let current_view = view();

    rsx! {
        style { "{MOBILE_UI_STYLES}" }
        div {
            style: "
                min-height: 100vh;
                font-family: system-ui, -apple-system, sans-serif;
                color: #111827;
                background: #f9fafb;
                display: flex;
                flex-direction: column;
            ",
            match current_view {
                View::List => rsx! { List {} },
                View::Editor => rsx! { Editor {} },
                View::Settings => rsx! { Settings {} },
            }
        }
    }
}
