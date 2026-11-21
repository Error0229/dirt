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
use std::time::{Duration, Instant};

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::{use_wry_event_handler, window, LogicalPosition};
use dioxus::prelude::*;
use dirt_core::config::ServerAddress;
use dirt_core::models::NoteId;
use dirt_core::session::{Session, SessionError, SessionState};
use dirt_core::sync::worker::{SyncStatus, Trigger};
use dirt_core::Store;
use dirt_ui::{Appearance, UiRoot};

use crate::bootstrap_config::load_bootstrap_config;
use crate::screen::writing::{Done, Scribe};
use crate::screen::MainScreen;
use crate::services::{data_folder, open_local_store};
use crate::settings::SettingsPanel;
use crate::state::AppState;
use crate::tray::Choice;

/// How long quitting, and taking another store into use, wait for the
/// store's queue to finish.
const QUEUE_WAITS_AT_MOST: Duration = Duration::from_secs(5);

/// How long taking another store into use, and quitting, wait for quick
/// capture to hand over what it holds: it is at once, unless its window
/// has not been made yet, when it holds nothing.
const HANDING_OVER_AT_MOST: Duration = Duration::from_secs(1);

/// Hands what the page, quick capture and a recording being kept hold to
/// the store each was written in, and waits for the store's queue. Before
/// signing out, so that the sign-out's sync takes it all to the server
/// (checklist D74 and D90), and before another store is taken into use,
/// which `then` does, once all is handed over and before the queue is
/// waited for: a thought typed meanwhile goes to the new store. The page
/// is left either way.
pub async fn hand_everything_over(
    page_leaver: CopyValue<Option<Callback<()>>>,
    mut current_note_id: Signal<Option<NoteId>>,
    then: impl FnOnce(),
) {
    let leave = *page_leaver.peek();
    if let Some(leave) = leave {
        leave.call(());
    }
    // Quick capture hands over to the store it was given when its first
    // character was typed, and a recording goes to the one in use as it
    // began.
    if !crate::capture::link().hand_over(HANDING_OVER_AT_MOST).await {
        tracing::warn!("Quick capture did not hand over what it held in time");
    }
    if !crate::voice::keeping_settled(QUEUE_WAITS_AT_MOST).await {
        tracing::warn!("A recording was not kept in time");
    }
    current_note_id.set(None);
    then();
    if !Scribe::global().settle(QUEUE_WAITS_AT_MOST).await {
        tracing::warn!("The store's queue did not finish in time");
    }
}

/// How many times the stores this device was signed out of are tried,
/// with a pause that doubles from 3 s to a minute, before what is left is
/// left for the next start. A sign-in held back by them is tried for as
/// long as it is held back.
const SWEEP_TRIES: u32 = 8;

/// Removes the stores this device was signed out of, here or by the CLI,
/// once nothing holds them: at once, and again while something does, a
/// list being loaded for a moment or the CLI waiting on an editor. A
/// sign-in that an old store kept from being taken into use
/// (`Session::held_back`) is taken into use as soon as it can be. One of
/// these runs at a time; it ends when nothing is held back and nothing is
/// left, or the tries run out.
fn remove_signed_out_stores_soon(session: Session) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        let mut wait = Duration::from_secs(3);
        for tries in 1.. {
            let left = session.remove_signed_out_stores();
            if session.held_back().is_some() {
                match session.reload() {
                    Ok(_) | Err(SessionError::PreviousNotesOpen) => {}
                    Err(error) => tracing::error!("Could not take the sign-in into use: {error}"),
                }
            }
            if session.held_back().is_none() && (!left || tries >= SWEEP_TRIES) {
                break;
            }
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(Duration::from_secs(60));
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
}

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
        // The window is made without the keyboard (`keyboard`) and shown
        // once its page has loaded: the person started the app, so it
        // takes the keyboard then.
        spawn(async {
            tokio::task::yield_now().await;
            if crate::keyboard::may_take() {
                window().set_focus();
            }
        });
    });
    let current_note_id = use_signal(|| None);
    let open_request = use_signal(|| None);
    let notes_read = use_signal(|| 0_u64);
    let pulled = use_signal(|| 0_u64);
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
                // The CLI may hold the notes of a sign-in it ended, which
                // kept this one from taking the new sign-in into use.
                remove_signed_out_stores_soon(opened_session.clone());
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
                    // What the page and quick capture hold goes to the
                    // queue, for the store it was written in. The new store
                    // is in use from now, so a thought typed meanwhile goes
                    // there; and the queue's work for the old one is done
                    // before anything lets go of it (on signing out it is
                    // removed below).
                    //
                    // Quick capture is given the new store first. What it
                    // hands over still goes to the old one: a note keeps
                    // the store in use when its first character was typed,
                    // and a recording the one in use as it began.
                    let store = Arc::new(store);
                    let capture = crate::capture::link();
                    capture.set_context(Some(Arc::clone(&store)), Some(current.clone()));
                    hand_everything_over(page_leaver, current_note_id, move || {
                        db_service.set(Some(store));
                    })
                    .await;
                    // The store let go of above may be one this device was
                    // signed out of, here or by the CLI, so its files can
                    // go now, and a sign-in it kept from being taken into
                    // use can be.
                    remove_signed_out_stores_soon(current.clone());
                }
            }
        }
    });

    // Follow sync: show its status, and read the notes again when a cycle
    // has brought notes from another device or sent some.
    dirt_ui::listing::use_sync_watch(session, sync_status, pulled, notes_read);

    // The window gaining focus is the person coming back to the app. The
    // CLI may have signed in or out, or written notes, in the meantime:
    // notice both, and sync unless that just happened.
    use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent {
            event: WindowEvent::Focused(true),
            ..
        } = event
        {
            if let Some(current) = session.peek().clone() {
                if let Err(error) = current.reload() {
                    tracing::error!("Could not re-read the sign-in: {error}");
                }
                current.notify(Trigger::Returned);
            }
            state.reread();
        }
    });

    // A write the store refused may have gone to a store this device was
    // signed out of by the CLI, which refuses every write from then on
    // (`dirt_core::Error::SignedOut`). The screen tells the person and
    // gives them their words; here the app notices the sign-out, and takes
    // the store now in use. Focus may never come, when the thought was
    // written in quick capture.
    use_future(move || async move {
        let mut news = Scribe::global().listen();
        loop {
            match news.recv().await {
                Ok(Done::Unsaved { .. } | Done::VoiceUnkept { .. }) => {
                    let signed_out = db_service
                        .peek()
                        .as_ref()
                        .is_some_and(|store| store.signed_out());
                    if !signed_out {
                        continue;
                    }
                    if let Some(current) = session.peek().clone() {
                        if let Err(error) = current.reload() {
                            tracing::error!("Could not re-read the sign-in: {error}");
                        }
                    }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
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
    // The microphone chosen in Settings, for the page and quick capture.
    use_effect(move || {
        crate::voice::choose_microphone(settings.read().microphone.clone());
    });

    // The tray's menu, and the hotkey, which goes to quick capture. Both
    // are heard here, in the main window's root, which is there for as
    // long as the app runs: the window is hidden, not closed.
    let choose = use_callback(move |choice| {
        tracing::info!("Tray: {choice:?} chosen");
        match choice {
            Choice::NewNote => crate::capture::link().open(Instant::now()),
            Choice::OpenDirt => {
                let win = window();
                win.set_visible(true);
                win.set_minimized(false);
                win.set_focus();
            }
            Choice::Quit => {
                // The main screen and quick capture hand over what their
                // pages hold as they hear of this; then the store's queue
                // is given a few seconds to finish, so that nothing typed
                // is lost.
                quitting.set(true);
                spawn(async move {
                    if !crate::capture::link().hand_over(HANDING_OVER_AT_MOST).await {
                        tracing::warn!("Quick capture did not hand over what it held in time");
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    // A recording being said is kept before the queue is
                    // waited for: it is only in memory until then.
                    if !crate::voice::keeping_settled(QUEUE_WAITS_AT_MOST).await {
                        tracing::warn!("Quitting with a recording not kept");
                    }
                    if !Scribe::global().settle(QUEUE_WAITS_AT_MOST).await {
                        tracing::warn!("Quitting with the store's work unfinished");
                    }
                    std::process::exit(0);
                });
            }
        }
    });
    crate::tray::use_tray(choose);
    // The tray's icon is the logo in the accent chosen in Settings.
    use_effect(move || crate::tray::show_accent(settings.read().accent));
    let hotkey = crate::hotkey::use_hotkey();
    crate::use_actions_from_stdin(hotkey, choose);

    // The words of what was said on this device, made in the background.
    crate::voice::use_words_maker(&state);
    // A web view that cannot play a recording as it is kept is handed it
    // decoded (6.18).
    use_hook(crate::voice::play_recordings_where_the_web_view_cannot);

    let appearance = Appearance::from_settings(&settings.read());

    rsx! {
        UiRoot { appearance,
            MainScreen {}
            SettingsPanel {}
        }
    }
}
