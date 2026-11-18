//! Main application component.
//!
//! The app holds one [`Session`], which says who is signed in, which note
//! store is in use and how sync is going. Without a server in the build
//! there is no session and notes stay in the local store.
//!
//! Everything is drawn inside `dirt_ui::UiRoot`, which brings the new
//! interface's style sheet and script and applies the theme, the accent
//! and the text size from this device's settings. The main screen is the
//! new one; Settings and quick capture are the old ones until their steps
//! replace them, and keep the old style sheets for that time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::{use_wry_event_handler, window, LogicalPosition, LogicalSize};
use dioxus::prelude::*;
use dirt_core::config::ServerAddress;
use dirt_core::session::{Session, SessionState};
use dirt_core::sync::worker::{SyncStatus, Trigger};
use dirt_core::Store;
use dirt_ui::{Appearance, UiRoot};

use crate::bootstrap_config::load_bootstrap_config;
use crate::components::{QuickCapture, SettingsPanel};
use crate::screen::writing::Scribe;
use crate::screen::MainScreen;
use crate::services::{data_folder, open_local_store};
use crate::state::{AppState, SettingsTab};
use crate::theme::{resolve_theme, ResolvedTheme};
use crate::tray::{process_tray_events, QUIT_REQUESTED, SHOW_MAIN_WINDOW};
use crate::{HOTKEY_TRIGGERED, TRAY_ENABLED};

/// How long quitting waits for the store's queue to finish.
const QUIT_WAITS_AT_MOST: Duration = Duration::from_secs(5);

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
/// a place off every screen works. A release build ignores it.
fn asked_position() -> Option<(f64, f64)> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let asked = std::env::var("DIRT_WINDOW_AT").ok()?;
    let (x, y) = asked.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
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
    let mut theme = use_signal(|| resolve_theme(dirt_core::models::ThemeMode::System));
    let settings_open = use_signal(|| false);
    let settings_tab = use_signal(SettingsTab::default);
    let mut quick_capture_open = use_signal(|| false);
    let mut quitting = use_signal(|| false);
    let mut saved_window_geometry: Signal<Option<(f64, f64, f64, f64)>> = use_signal(|| None);
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
        theme,
        db_service,
        device_store,
        startup_error,
        session,
        session_state,
        sync_status,
        settings_open,
        settings_tab,
        quick_capture_open,
        quitting,
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
            theme.set(resolve_theme(loaded_settings.theme));
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
                    current_note_id.set(None);
                    db_service.set(Some(Arc::new(store)));
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

    // Poll for the hotkey, the tray and the window coming back into focus.
    use_future(move || async move {
        let tray_enabled = TRAY_ENABLED.load(Ordering::SeqCst);
        loop {
            // Process tray menu events
            if tray_enabled {
                process_tray_events();

                // Check for show window request
                if SHOW_MAIN_WINDOW.swap(false, Ordering::SeqCst) {
                    tracing::info!("Showing main window from tray");
                    let win = window();
                    let tao_win = &win.window;

                    // Restore pre-capture geometry before showing.
                    if let Some((w, h, x, y)) = saved_window_geometry() {
                        tao_win.set_outer_position(LogicalPosition::new(x, y));
                        tao_win.set_inner_size(LogicalSize::new(w, h));
                        saved_window_geometry.set(None);
                    }

                    quick_capture_open.set(false);
                    win.set_visible(true);
                    win.set_focus();
                }

                // Check for quit request
                if QUIT_REQUESTED.swap(false, Ordering::SeqCst) {
                    tracing::info!("Quit requested from tray");
                    // The main screen hands over what its page holds as it
                    // hears of this; then the store's queue is given a few
                    // seconds to finish, so that nothing typed is lost.
                    quitting.set(true);
                    spawn(async move {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        if !Scribe::global().settle(QUIT_WAITS_AT_MOST).await {
                            tracing::warn!("Quitting with the store's work unfinished");
                        }
                        std::process::exit(0);
                    });
                }
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

            // Check if hotkey was triggered
            if HOTKEY_TRIGGERED.swap(false, Ordering::SeqCst) {
                tracing::info!("Opening quick capture");
                let win = window();
                let tao_win = &win.window;

                // Save main-window geometry once; keep it across repeated captures
                // until the main window is explicitly reopened.
                if saved_window_geometry().is_none() {
                    let scale = tao_win.current_monitor().map_or(1.0, |m| m.scale_factor());
                    let phys_size = tao_win.inner_size();
                    let phys_pos = tao_win.outer_position().unwrap_or_default();
                    saved_window_geometry.set(Some((
                        f64::from(phys_size.width) / scale,
                        f64::from(phys_size.height) / scale,
                        f64::from(phys_pos.x) / scale,
                        f64::from(phys_pos.y) / scale,
                    )));
                }

                // Resize to compact quick capture size
                let capture_w = 420.0;
                let capture_h = 200.0;
                tao_win.set_inner_size(LogicalSize::new(capture_w, capture_h));

                // Center on current monitor
                if let Some(monitor) = tao_win.current_monitor() {
                    let mon_size = monitor.size();
                    let mon_pos = monitor.position();
                    let mon_scale = monitor.scale_factor();
                    let cx = f64::from(mon_pos.x) / mon_scale
                        + (f64::from(mon_size.width) / mon_scale - capture_w) / 2.0;
                    let cy = f64::from(mon_pos.y) / mon_scale
                        + (f64::from(mon_size.height) / mon_scale - capture_h) / 2.0;
                    tao_win.set_outer_position(LogicalPosition::new(cx, cy));
                }

                win.set_visible(true);
                win.set_focus();
                quick_capture_open.set(true);
            }

            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    });

    let appearance = Appearance::from_settings(&settings.read());
    // The old settings panel and quick capture take their colours from
    // the old style sheets, by this attribute.
    let legacy_theme = match theme() {
        ResolvedTheme::Light => "light",
        ResolvedTheme::Dark => "dark",
    };

    rsx! {
        // The old style sheets, for Settings and quick capture until their
        // steps replace them.
        document::Link {
            rel: "stylesheet",
            href: asset!("/assets/dx-components-theme.css"),
        }
        document::Link { rel: "stylesheet", href: asset!("/assets/theme-overrides.css") }

        UiRoot { appearance,
            if quick_capture_open() {
                div { class: "dk-legacy", "data-theme": legacy_theme, QuickCapture {} }
            } else {
                MainScreen {}
                if settings_open() {
                    div { class: "dk-legacy", "data-theme": legacy_theme, SettingsPanel {} }
                }
            }
        }
    }
}
