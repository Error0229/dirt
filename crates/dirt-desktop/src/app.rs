//! Main application component.
//!
//! The app holds one [`Session`], which says who is signed in, which note
//! store is in use and how sync is going. Without a server in the build
//! there is no session and notes stay in the local store.
//!
//! Everything is drawn inside `dirt_ui::UiRoot`, which brings the new
//! interface's style sheet and script and applies the theme, the accent
//! and the text size from this device's settings. Quick capture is a
//! window of its own (`capture`), made hidden beside this one as the app
//! starts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::{use_wry_event_handler, window, LogicalPosition};
use dioxus::prelude::*;
use dirt_core::config::ServerAddress;
use dirt_core::session::{Session, SessionState};
use dirt_core::sync::worker::{SyncStatus, Trigger};
use dirt_core::Store;
use dirt_ui::{Appearance, UiRoot};

use crate::bootstrap_config::load_bootstrap_config;
use crate::screen::writing::Scribe;
use crate::screen::MainScreen;
use crate::services::{data_folder, open_local_store};
use crate::settings::SettingsPanel;
use crate::state::AppState;
use crate::tray::{process_tray_events, QUIT_REQUESTED, SHOW_MAIN_WINDOW};
use crate::TRAY_ENABLED;

/// How long quitting, and taking another store into use, wait for the
/// store's queue to finish.
const QUEUE_WAITS_AT_MOST: Duration = Duration::from_secs(5);

/// Set when the window gains focus: the person has come back to the app.
static WINDOW_FOCUSED: AtomicBool = AtomicBool::new(false);

/// The stores the app starts with, and the session if the build has a
/// server to sync with.
struct Opened {
    notes: Store,
    device: Store,
    session: Option<Session>,
}

fn open_stores(server_url: Option<&str>) -> Result<Opened, String> {
    if let Some(url) = server_url {
        let session = Session::on_this_computer(url, data_folder(), true)
            .map_err(|error| error.to_string())?;
        Ok(Opened {
            notes: session.store(),
            device: session.device_store(),
            session: Some(session),
        })
    } else {
        let local = open_local_store().map_err(|error| error.to_string())?;
        Ok(Opened {
            notes: local.clone(),
            device: local,
            session: None,
        })
    }
}

/// Where `DIRT_WINDOW_AT=x,y` asks for the window's top left corner, in
/// screen points, in a development build. That is for looking at the app
/// without its window covering what the person at the computer is doing:
/// a place off every screen works; quick capture then opens there too. A
/// release build ignores it.
pub fn asked_position() -> Option<(f64, f64)> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let asked = std::env::var("DIRT_WINDOW_AT").ok()?;
    let (x, y) = asked.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Whether the app's windows may take the keyboard from the app in front:
/// quick capture as it opens, the main window coming back for an error. Not
/// in a development build kept off every screen (`DIRT_WINDOW_AT`): a
/// window nobody can see must not take the keys of the person at the
/// computer, nor ask for their attention.
pub fn takes_keyboard() -> bool {
    asked_position().is_none()
}

/// Root application component
#[component]
#[allow(clippy::too_many_lines)]
pub fn App() -> Element {
    // A window cannot be made at a place that is on no screen, but it can
    // be moved there: the first render comes before the window is shown.
    use_hook(|| {
        if let Some((x, y)) = asked_position() {
            window().set_outer_position(LogicalPosition::new(x, y));
        }
    });
    let mut current_note_id = use_signal(|| None);
    let open_request = use_signal(|| None);
    let notes_read = use_signal(|| 0_u64);
    let mut pulled = use_signal(|| 0_u64);
    let mut settings = use_signal(dirt_core::models::Settings::default);
    let nav = dirt_ui::settings::use_settings_nav();
    let mut quitting = use_signal(|| false);
    let page_leaver = use_hook(|| CopyValue::new(None::<Callback<()>>));
    let mut db_service: Signal<Option<Arc<Store>>> = use_signal(|| None);
    let mut device_store: Signal<Option<Arc<Store>>> = use_signal(|| None);
    let mut startup_error: Signal<Option<String>> = use_signal(|| None);
    let mut session: Signal<Option<Session>> = use_signal(|| None);
    let mut session_state = use_signal(|| SessionState::SignedOut);
    let mut sync_status = use_signal(SyncStatus::default);
    // The server is `DIRT_API_BASE_URL` in the environment, or else the
    // address baked into the build: the rule the CLI follows too, because
    // the two are one device. It does not change while the app runs.
    let server_url =
        ServerAddress::on_this_computer(load_bootstrap_config().dirt_api_base_url.as_deref())
            .map(|address| address.map(|address| address.url))
            .map_err(|error| dirt_core::util::as_sentence(&error));

    let state = use_context_provider(|| AppState {
        current_note_id,
        open_request,
        notes_read,
        pulled,
        settings,
        db_service,
        device_store,
        startup_error,
        session,
        session_state,
        sync_status,
        nav,
        quitting,
        page_leaver,
    });

    // Open the stores, and the session if there is a server, then load
    // this device's settings.
    let _db_init_task = use_resource(move || {
        let server_url = server_url.clone();
        async move {
            // An address that cannot be one stops the app: carrying on
            // without a server, or with another, is not what was asked for.
            let server_url = match server_url {
                Ok(url) => url,
                Err(problem) => {
                    tracing::error!("{problem}");
                    startup_error.set(Some(problem));
                    return;
                }
            };
            let opened = match open_stores(server_url.as_deref()) {
                Ok(opened) => opened,
                Err(error) => {
                    tracing::error!("Failed to open the note store: {error}");
                    startup_error.set(Some(format!("Could not open your notes: {error}")));
                    return;
                }
            };
            let loaded_settings = match opened.device.load_settings().await {
                Ok(settings) => settings,
                Err(error) => {
                    tracing::error!("Failed to load desktop settings: {error}");
                    startup_error.set(Some(format!("Could not read your settings: {error}")));
                    return;
                }
            };
            settings.set(loaded_settings);

            device_store.set(Some(Arc::new(opened.device)));
            db_service.set(Some(Arc::new(opened.notes)));
            if let Some(opened_session) = &opened.session {
                session_state.set(opened_session.state());
                sync_status.set(opened_session.sync_status());
            }
            session.set(opened.session);
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
                let store = current.store();
                let same_store = db_service
                    .peek()
                    .as_ref()
                    .is_some_and(|open| open.path() == store.path());
                if !same_store {
                    // What the page holds goes to the queue, for the store
                    // it was written in. The new store is in use from now,
                    // so a thought typed meanwhile goes there; and the
                    // queue's work for the old one is done before anything
                    // lets go of it (on signing out it is removed below).
                    let leave = *page_leaver.peek();
                    if let Some(leave) = leave {
                        leave.call(());
                    }
                    current_note_id.set(None);
                    db_service.set(Some(Arc::new(store)));
                    if !Scribe::global().settle(QUEUE_WAITS_AT_MOST).await {
                        tracing::warn!("Took another store into use with work unfinished");
                    }
                }
                if state_now == SessionState::SignedOut {
                    // The account's store has been let go of above, so its
                    // files can go now. Something may still be reading it
                    // for a moment, a list being loaded for one, so it is
                    // tried again shortly. What is still there after that
                    // goes when the app next starts.
                    current.remove_signed_out_stores();
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    current.remove_signed_out_stores();
                }
            }
        }
    });

    // Follow sync: show its status, and read the notes again when a cycle
    // has brought notes from another device or sent some.
    let _sync_watch = use_resource(move || {
        let current = session();
        async move {
            let Some(current) = current else { return };
            let mut statuses = current.watch_sync();
            let mut pulled_before = statuses.borrow().pulled;
            let mut synced_before = statuses.borrow().last_synced_ms;
            while statuses.changed().await.is_ok() {
                let status = statuses.borrow_and_update().clone();
                if status.pulled != pulled_before {
                    pulled_before = status.pulled;
                    *pulled.write() += 1;
                    state.reread();
                } else if status.last_synced_ms != synced_before {
                    // A cycle that only sent: the notes it sent are on the
                    // server now, and the list stops marking them.
                    state.reread();
                }
                synced_before = status.last_synced_ms;
                sync_status.set(status);
            }
        }
    });

    // The window gaining focus is the person coming back to the app.
    use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent {
            event: WindowEvent::Focused(true),
            ..
        } = event
        {
            WINDOW_FOCUSED.store(true, Ordering::SeqCst);
        }
    });

    // Quick capture's window, made now and kept hidden until it is asked
    // for, so that it shows at once. It writes into the store in use, in
    // the looks chosen here.
    use_hook(crate::capture::make_window);
    use_effect(move || {
        crate::capture::link().set_context(db_service(), session());
    });
    use_effect(move || {
        crate::capture::link().set_appearance(Appearance::from_settings(&settings.read()));
    });

    // Poll for the tray, a request to show the window and the window
    // coming back into focus. The hotkey goes to quick capture directly.
    use_future(move || async move {
        let tray_enabled = TRAY_ENABLED.load(Ordering::SeqCst);
        loop {
            if tray_enabled {
                process_tray_events();
            }

            if SHOW_MAIN_WINDOW.swap(false, Ordering::SeqCst) {
                tracing::info!("Showing main window from tray");
                let win = window();
                win.set_visible(true);
                win.set_minimized(false);
                win.set_focus();
            }

            if QUIT_REQUESTED.swap(false, Ordering::SeqCst) {
                tracing::info!("Quit requested from tray");
                // The main screen and quick capture hand over what their
                // pages hold as they hear of this; then the store's queue
                // is given a few seconds to finish, so that nothing typed
                // is lost.
                quitting.set(true);
                crate::capture::link().leave();
                spawn(async move {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if !Scribe::global().settle(QUEUE_WAITS_AT_MOST).await {
                        tracing::warn!("Quitting with the store's work unfinished");
                    }
                    std::process::exit(0);
                });
            }

            // The person came back to the app. The CLI may have signed in or
            // out, or written notes, in the meantime: notice both, and sync
            // unless that just happened.
            if WINDOW_FOCUSED.swap(false, Ordering::SeqCst) {
                if let Some(current) = session.peek().clone() {
                    if let Err(error) = current.reload() {
                        tracing::error!("Could not re-read the sign-in: {error}");
                    }
                    current.notify(Trigger::Returned);
                }
                state.reread();
            }

            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    });

    let appearance = Appearance::from_settings(&settings.read());

    rsx! {
        UiRoot { appearance,
            MainScreen {}
            SettingsPanel {}
        }
    }
}
