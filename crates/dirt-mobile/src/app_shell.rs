//! Mobile app shell.
//!
//! Owns start-up: it opens the session, which says who is signed in and
//! which note store is in use, and keeps the interface following it. A build
//! with no server, or a device whose secure storage cannot be used, has no
//! session and keeps its notes in the local store.
//!
//! Signing in leaves the app for the browser. Android brings the person
//! back through the custom address `dev.dirt.app://callback`, which
//! `MainActivity` keeps until this code asks for it (`services::sign_in`).
//! Android may stop the app while the browser is in front, so the redirect
//! is also asked for at start-up; the session saved the attempt before the
//! browser opened.
//!
//! The activity is asked in the same way for what came from outside the
//! app: text or pictures another app shared, which become a note at once,
//! and a press of the home-screen widget, which opens a blank note.

use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::api::HttpTransport;
use dirt_core::config::ServerAddress;
use dirt_core::models::{Note, NoteId};
use dirt_core::secrets::slot_name;
use dirt_core::session::{Session, SessionConfig, SessionError, SessionState};
use dirt_core::store::StoreName;
use dirt_core::sync::worker::{SyncStatus, Timing, Trigger};
use dirt_core::Store;

use crate::bootstrap_config::load_bootstrap_config;
use crate::config::default_mobile_data_directory;
use crate::data::{MobileNoteStore, NOTES_PAGE};
use crate::incoming::{
    attach_incoming, discard_all, not_copied_in, note_text, worth_a_note, Capture,
};
use crate::services::activity::{take_capture, take_redirect};
use crate::services::secure_storage::EncryptedPrefs;
use crate::state::{AppState, View};
use crate::ui::MOBILE_UI_STYLES;
use crate::views::{Devices, Editor, List, Settings};

/// Where the sign-in service sends the browser afterwards. The manifest
/// registers the app for this address.
pub const REDIRECT_URI: &str = "dev.dirt.app://callback";

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
    Store::open_in(&default_mobile_data_directory(), &StoreName::Local)
        .map_err(|error| error.to_string())
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

/// Deals with what was asked of the app from outside it, if anything was.
///
/// What was shared is saved as a note straight away: capture does not
/// wait for the person, who finds the note at the top of the list. Shared
/// pictures are attached to it, and a note made of pictures alone is called
/// after them. The widget asks for a blank note to write in. Neither takes
/// the person out of a note they are in the middle of writing.
async fn receive_capture(
    store: Signal<Option<Arc<MobileNoteStore>>>,
    session: Signal<Option<Session>>,
    mut notes: Signal<Vec<Note>>,
    listed_notes: Signal<usize>,
    mut selected_note_id: Signal<Option<NoteId>>,
    mut view: Signal<View>,
    mut startup_issue: Signal<Option<String>>,
) {
    // More than one may be waiting: two shares can arrive before either is
    // asked for.
    while let Some(capture) = take_capture() {
        match capture {
            Capture::Blank => {
                if *view.peek() != View::Editor {
                    selected_note_id.set(None);
                    view.set(View::Editor);
                }
            }
            Capture::Shared { text, files } => {
                if !worth_a_note(&text, &files) {
                    // Nothing came in to make a note of. Say what was lost.
                    startup_issue.set(Some(not_copied_in(&files).join(" ")));
                    continue;
                }
                let Some(note_store) = store.peek().clone() else {
                    discard_all(&files);
                    startup_issue.set(Some(
                        "What was shared could not be saved: the notes are not open.".to_owned(),
                    ));
                    continue;
                };
                match note_store.create_note(&note_text(&text, &files)).await {
                    Ok(note) => {
                        // As a task of its own: it finishes whatever the
                        // screen does meanwhile.
                        let core = note_store.core().clone();
                        let failed = tokio::spawn(attach_incoming(core, note.id, files))
                            .await
                            .unwrap_or_default();
                        if !failed.is_empty() {
                            startup_issue.set(Some(failed.join(" ")));
                        }
                        if let Some(current) = session.peek().as_ref() {
                            current.notify(Trigger::Changed);
                        }
                        let listed = *listed_notes.peek();
                        match note_store.list_notes(listed).await {
                            Ok(refreshed) => notes.set(refreshed),
                            Err(error) => tracing::warn!("Listing after a share failed: {error}"),
                        }
                        // Asked now, not before the note was saved: the
                        // person may have begun writing in the meantime.
                        if *view.peek() != View::Editor {
                            view.set(View::List);
                        }
                    }
                    Err(error) => {
                        discard_all(&files);
                        tracing::error!("What was shared could not be saved: {error}");
                        startup_issue
                            .set(Some(format!("What was shared could not be saved: {error}")));
                    }
                }
            }
        }
    }
}

#[component]
pub fn AppShell() -> Element {
    // The address the app was built with. One that cannot be an address
    // is said at start-up and not passed over.
    let server_url =
        ServerAddress::on_android(load_bootstrap_config().dirt_api_base_url.as_deref())
            .map(|address| address.map(|address| address.url))
            .map_err(|error| dirt_core::util::as_sentence(&error));

    let mut notes = use_signal(Vec::new);
    let mut selected_note_id = use_signal(|| None);
    let mut view = use_signal(|| View::List);
    let search_query = use_signal(String::new);
    let tag_filter = use_signal(|| None::<String>);
    let listed_notes = use_signal(|| NOTES_PAGE);
    let mut startup_issue = use_signal(|| None::<String>);
    let mut store: Signal<Option<Arc<MobileNoteStore>>> = use_signal(|| None);
    let mut session: Signal<Option<Session>> = use_signal(|| None);
    let mut session_state = use_signal(|| SessionState::SignedOut);
    let mut sync_status = use_signal(SyncStatus::default);

    use_context_provider(|| AppState {
        notes,
        selected_note_id,
        view,
        search_query,
        tag_filter,
        listed_notes,
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
            // Copies of files lent to other apps in earlier runs, and
            // exports that were never saved.
            dirt_core::files::clear_copies(&crate::views::attachments::opened_folder());
            crate::export::clear_exports(&crate::export::export_folder());
            let server_url = match server_url {
                Ok(url) => url,
                Err(problem) => {
                    tracing::error!("{problem}");
                    startup_issue.set(Some(problem));
                    None
                }
            };
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
            let listed = *listed_notes.peek();
            match note_store.list_notes(listed).await {
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

            // The app may have been started by a share or by the widget.
            receive_capture(
                store,
                session,
                notes,
                listed_notes,
                selected_note_id,
                view,
                startup_issue,
            )
            .await;
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
                    let listed = *listed_notes.peek();
                    notes.set(next.list_notes(listed).await.unwrap_or_default());
                    store.set(Some(next));
                }
                if state == SessionState::SignedOut {
                    current.remove_signed_out_stores();
                    // Copies of the account's files that were lent to other
                    // apps go with the account, and so does an export of
                    // its notes that is still waiting to be saved.
                    dirt_core::files::clear_copies(&crate::views::attachments::opened_folder());
                    crate::export::clear_exports(&crate::export::export_folder());
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
                        let listed = *listed_notes.peek();
                        if let Ok(latest) = open.list_notes(listed).await {
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
            // The activity says when something arrived while the page was
            // visible all along.
            window.addEventListener("dirt-activity", () => dioxus.send(true));
            // Stay alive, so that the messages above keep arriving.
            await new Promise(() => {});
            "#,
        );
        while became_visible.recv::<bool>().await.is_ok() {
            if let Some(current) = session.peek().clone() {
                current.notify(Trigger::Returned);
            }
            // A share or the widget may be what brought the app forward.
            receive_capture(
                store,
                session,
                notes,
                listed_notes,
                selected_note_id,
                view,
                startup_issue,
            )
            .await;
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
                View::Devices => rsx! { Devices {} },
            }
        }
    }
}
