//! Mobile app shell.
//!
//! Owns start-up: it opens the session, which says who is signed in and
//! which note store is in use, and keeps the interface following it. A build
//! with no server, or a device whose secure storage cannot be used, has no
//! session and keeps its notes in the local store.
//!
//! Everything is drawn inside `dirt_ui::UiRoot`, which brings the new
//! interface's style sheet and script and applies the theme, the accent
//! and the text size from this device's settings. The main screen is always
//! there; Settings is a sheet over it (`crate::settings`).
//!
//! Signing in leaves the app for the browser. Android brings the person
//! back through the custom address `dev.dirt.app://callback`, which
//! `MainActivity` keeps until this code asks for it (`services::sign_in`).
//! Android may stop the app while the browser is in front, so the redirect
//! is also asked for at start-up; the session saved the attempt before the
//! browser opened.
//!
//! The activity is asked in the same way for what another app shared,
//! text or pictures, which become a note at once. The main screen does the
//! asking (`screen::Screen`); this says when to.
//!
//! The capture sheet (`capture`) keeps its thoughts in files of their own,
//! and a job of its own makes them notes. This takes in whatever still
//! waits as the app starts and each time the person returns to it, and
//! reads the list again, since a note the job made is in the store already
//! and nothing else tells the list of it. A recording the sheet kept is
//! taken in the same way, and its words are made then (`voice`).

use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::api::HttpTransport;
use dirt_core::config::ServerAddress;
use dirt_core::models::Settings;
use dirt_core::secrets::slot_name;
use dirt_core::session::{Session, SessionConfig, SessionError, SessionState};
use dirt_core::store::StoreName;
use dirt_core::sync::worker::{SyncStatus, Timing, Trigger};
use dirt_core::Store;
use dirt_ui::{Appearance, UiRoot};

use crate::bootstrap_config::load_bootstrap_config;
use crate::capture::{take_in, write_look};
use crate::config::default_mobile_data_directory;
use crate::data::MobileNoteStore;
use crate::screen::MainScreen;
use crate::services::activity::{animations_off, screen_reader_on, take_redirect};
use crate::services::secure_storage::EncryptedPrefs;
use crate::settings::SettingsSheet;
use crate::state::AppState;

/// Where the sign-in service sends the browser afterwards. The manifest
/// registers the app for this address.
pub const REDIRECT_URI: &str = "dev.dirt.app://callback";

/// The name other devices see for this one.
const DEVICE_NAME: &str = "Android device";

fn open_session(server_url: &str) -> Result<Session, String> {
    let secrets =
        EncryptedPrefs::open(&slot_name(server_url)).map_err(|error| error.to_string())?;
    session_with(server_url, secrets, default_mobile_data_directory(), true)
}

/// The device's session with the server at `server_url`, its secrets in
/// `secrets` and its stores under `data_folder`: the app's, which syncs in
/// the background, or the capture job's, which syncs once (`capture`).
pub fn session_with(
    server_url: &str,
    secrets: EncryptedPrefs,
    data_folder: std::path::PathBuf,
    background_sync: bool,
) -> Result<Session, String> {
    let transport = HttpTransport::new(server_url).map_err(|error| error.to_string())?;
    Session::open(
        SessionConfig {
            data_folder,
            device_name: DEVICE_NAME.to_owned(),
            background_sync,
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

#[component]
#[allow(clippy::too_many_lines)]
pub fn AppShell() -> Element {
    // The address the app was built with. One that cannot be an address
    // is said at start-up and not passed over.
    let server_url =
        ServerAddress::on_android(load_bootstrap_config().dirt_api_base_url.as_deref())
            .map(|address| address.map(|address| address.url))
            .map_err(|error| dirt_core::util::as_sentence(&error));

    let nav = dirt_ui::settings::use_settings_nav();
    let mut startup_issue = use_signal(|| None::<String>);
    let mut store: Signal<Option<Arc<MobileNoteStore>>> = use_signal(|| None);
    let mut device_store: Signal<Option<Store>> = use_signal(|| None);
    let mut settings = use_signal(Settings::default);
    let mut reduced_motion = use_signal(animations_off);
    use_hook(|| dirt_ui::components::set_screen_reader(screen_reader_on()));
    let mut session: Signal<Option<Session>> = use_signal(|| None);
    let mut session_state = use_signal(|| SessionState::SignedOut);
    let mut sync_status = use_signal(SyncStatus::default);
    let notes_read = use_signal(|| 0_u64);
    let pulled = use_signal(|| 0_u64);
    let mut current_note_id = use_signal(|| None);
    let open_request = use_signal(|| None);
    let mut capture_ready = use_signal(|| 0_u64);

    let state = use_context_provider(|| AppState {
        nav,
        startup_issue,
        store,
        device_store,
        settings,
        session,
        session_state,
        sync_status,
        notes_read,
        pulled,
        current_note_id,
        open_request,
        capture_ready,
    });

    // One-shot start-up. `use_resource` only runs again if a signal it
    // read changes, and it reads none.
    let _init = use_resource(move || {
        let server_url = server_url.clone();
        async move {
            // Copies of files lent to other apps in earlier runs, and
            // exports that were never saved.
            dirt_core::files::clear_copies(&crate::files::opened_folder());
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
            // The settings are the device's, kept in its local store
            // whoever is signed in.
            let (note_store, device) = opened_session.as_ref().map_or_else(
                || match open_local_store() {
                    Ok(local) => (Ok(local.clone()), Some(local)),
                    Err(error) => (Err(error), None),
                },
                |opened| (Ok(opened.store()), Some(opened.device_store())),
            );
            if let Some(device) = &device {
                match device.load_settings().await {
                    Ok(loaded) => settings.set(loaded),
                    Err(error) => tracing::warn!("Could not read this device's settings: {error}"),
                }
            }
            device_store.set(device);
            let note_store = match note_store {
                Ok(note_store) => Arc::new(MobileNoteStore::new(note_store)),
                Err(error) => {
                    tracing::error!("Failed to open mobile DB: {error}");
                    startup_issue.set(Some(format!("Failed to open local database: {error}")));
                    return;
                }
            };
            // Thoughts the capture sheet left waiting become notes now, if
            // its job has not made them already, and are sent: the sync the
            // session began as it opened may have run before they were made.
            let recorder = crate::voice::app_recorder();
            let taken = take_in(
                note_store.core(),
                &default_mobile_data_directory(),
                recorder.as_ref(),
            )
            .await;
            if taken.made > 0 {
                if let Some(opened) = &opened_session {
                    opened.notify(Trigger::Changed);
                }
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
            *capture_ready.write() += 1;
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
                let state_now = *states.borrow_and_update();
                session_state.set(state_now);
                let next = current.store();
                let same_store = store
                    .peek()
                    .as_ref()
                    .is_some_and(|open| open.path() == next.path());
                if !same_store {
                    current_note_id.set(None);
                    store.set(Some(Arc::new(MobileNoteStore::new(next))));
                }
                if state_now == SessionState::SignedOut {
                    current.remove_signed_out_stores();
                    // Copies of the account's files that were lent to other
                    // apps go with the account, and so does an export of
                    // its notes that is still waiting to be saved.
                    dirt_core::files::clear_copies(&crate::files::opened_folder());
                    crate::export::clear_exports(&crate::export::export_folder());
                }
            }
        }
    });

    // Follow sync: show its status, and read the notes again when a cycle
    // has brought notes from another device or sent some.
    dirt_ui::listing::use_sync_watch(session, sync_status, pulled, notes_read);

    // While a sign-in is under way, watch for the redirect the browser
    // sends the person back with.
    let _redirect_watch = use_resource(move || {
        let state_now = session_state();
        let current = session();
        async move {
            let Some(current) = current else { return };
            if state_now != SessionState::SigningIn {
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
    // skips the sync if one ran a moment ago. "Remove animations" may have
    // been changed meanwhile, and is read again.
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
            // The capture sheet, or its job, may have written while the app
            // was away. What still waits is taken in, and the list is read
            // again either way.
            let open = store.peek().clone();
            if let Some(open) = open {
                let recorder = crate::voice::app_recorder();
                let taken = take_in(
                    open.core(),
                    &default_mobile_data_directory(),
                    recorder.as_ref(),
                )
                .await;
                if taken.made > 0 {
                    if let Some(current) = session.peek().clone() {
                        current.notify(Trigger::Changed);
                    }
                }
                // Words that were left to make, the sheet's job's or this
                // app's from before, are made now.
                crate::voice::make_words_soon();
                state.reread();
            }
            if let Some(current) = session.peek().clone() {
                current.notify(Trigger::Returned);
            }
            let off = animations_off();
            if *reduced_motion.peek() != off {
                reduced_motion.set(off);
            }
            dirt_ui::components::set_screen_reader(screen_reader_on());
            // A share or the widget may be what brought the app forward.
            *capture_ready.write() += 1;
        }
    });

    // The words of what was said on this phone, made in the background.
    crate::voice::use_words_maker(&state);

    // The capture sheet and the widget are drawn outside the web view, in
    // the theme and the accent chosen here: they read them from a file.
    use_effect(move || {
        let chosen = settings.read();
        if let Err(error) = write_look(
            &default_mobile_data_directory(),
            chosen.theme,
            chosen.accent,
        ) {
            tracing::warn!("Could not write the look for the capture sheet: {error}");
        }
    });

    let appearance = Appearance::from_settings(&settings.read());

    rsx! {
        UiRoot { appearance, reduced_motion: reduced_motion(),
            MainScreen {}
            SettingsSheet {}
        }
    }
}
