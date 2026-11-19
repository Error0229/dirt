//! Dirt Desktop Application
//!
//! A cross-platform desktop app for capturing fleeting thoughts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Dioxus `asset!` macro expansion triggers false positives for
// `clippy::volatile_composites` on newer toolchains.
#![allow(unknown_lints, clippy::volatile_composites)]

mod app;
mod bootstrap_config;
mod capture;
#[cfg(feature = "gallery")]
mod gallery;
mod hotkey;
mod keyboard;
mod screen;
mod services;
mod settings;
mod state;
mod tray;
mod voice;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use dioxus::desktop::{Config, WindowCloseBehaviour};
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use hotkey::HotkeyManager;
use single_instance::SingleInstance;

/// Atomic flag indicating tray is enabled
pub static TRAY_ENABLED: AtomicBool = AtomicBool::new(false);

#[allow(clippy::cognitive_complexity)]
fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=debug".parse().unwrap()),
        )
        .init();

    // Built to show the interface's gallery, this is not the app: it has
    // no tray, no hotkey and no notes.
    #[cfg(feature = "gallery")]
    if cfg!(feature = "gallery") {
        gallery::launch();
        return;
    }

    tracing::info!("Starting Dirt...");

    let _single_instance = match SingleInstance::new("dirt-desktop-single-instance") {
        Ok(instance) if instance.is_single() => instance,
        Ok(_) => {
            tracing::error!("Another Dirt desktop instance is already running.");
            return;
        }
        Err(error) => {
            tracing::error!("Failed to initialize single-instance guard: {}", error);
            return;
        }
    };

    // Initialize system tray BEFORE Dioxus (must be on main thread)
    match tray::start() {
        Ok(()) => {
            tracing::info!("System tray initialized");
            TRAY_ENABLED.store(true, Ordering::SeqCst);
        }
        Err(e) => {
            tracing::error!("Failed to initialize system tray: {}", e);
        }
    }

    // Initialize global hotkey BEFORE launching Dioxus.
    // The manager must be kept alive and stay on the main thread.
    // Retry briefly to tolerate OS-level release delay from a previous instance.
    let _hotkey_manager = match initialize_hotkey_manager() {
        Ok(manager) => Some(manager),
        Err(e) => {
            tracing::warn!("Failed to register hotkey after retries: {}", e);
            tracing::warn!("Continuing desktop startup without global hotkey support.");
            None
        }
    };

    take_actions_from_stdin();

    // Copies of files handed to other programs in earlier runs.
    services::clear_opened_copies(&services::data_folder());

    // Configure Dioxus to hide window on close instead of exiting
    // Hide window instead of exiting when closed - keeps app running in tray
    let config = without_the_web_views_own_keys(
        Config::new()
            .with_window(screen::window::builder())
            .with_close_behaviour(WindowCloseBehaviour::WindowHides),
    );

    // Launch the app
    dioxus::LaunchBuilder::new()
        .with_cfg(config)
        .launch(app::App);
}

/// Turns off what the web view would do with a browser's keys.
///
/// That is reload, find, print, zoom, developer tools, and a browser's menu
/// on a right click (interface design, sections 7.4 and 10). Release builds
/// only; a development build keeps reload and the developer tools.
///
/// Most of it is the web view's settings, which Dioxus and wry set:
///
/// - On Windows wry turns the web view's browser accelerator keys off for every
///   build (`AreBrowserAcceleratorKeysEnabled`: Ctrl R and F5, Ctrl F,
///   Ctrl P, Ctrl + and Ctrl -, F12), and leaves zoom by Ctrl and the
///   wheel off unless asked (`IsZoomControlEnabled`).
/// - Turning the context menu off here also keeps the developer tools out
///   (`AreDevToolsEnabled` on Windows, the inspector on Linux).
///
/// The script in the page's head stops the same keys for a web view whose
/// settings do not, which is the case on Linux and on a Mac for some of
/// them, and lets each key go on to the app.
pub fn without_the_web_views_own_keys(config: Config) -> Config {
    if !dirt_ui::webview::own_keys_off() {
        return config;
    }
    config
        .with_disable_context_menu(true)
        .with_custom_head(dirt_ui::webview::guard_head())
}

/// How long Ctrl Alt N is held before quick capture listens instead of
/// waiting for typing (6.18, 7.2): the mic's line between a tap and a hold.
const HELD_AFTER: Duration = Duration::from_millis(280);

/// The keys are down: from the press until they are let go.
static HOTKEY_DOWN: AtomicBool = AtomicBool::new(false);

/// Counts presses, so that a hold is told apart from the press before.
static PRESSES: AtomicU64 = AtomicU64::new(0);

/// This press has been held long enough to talk. Only then does letting
/// go say anything: quick capture keeps only the latest thing asked of it,
/// and a quick tap's letting go would otherwise take the place of its
/// opening.
static TALKING: AtomicBool = AtomicBool::new(false);

/// Ctrl Alt N was pressed: quick capture opens, for typing. Held past
/// [`HELD_AFTER`], it listens instead. The system repeats a key held down;
/// a repeat is not another press.
fn hotkey_pressed() {
    if HOTKEY_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    tracing::info!("Hotkey pressed (Ctrl+Alt+N)");
    capture::link().open(Instant::now());
    let press = PRESSES.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        thread::sleep(HELD_AFTER);
        if HOTKEY_DOWN.load(Ordering::SeqCst) && PRESSES.load(Ordering::SeqCst) == press {
            TALKING.store(true, Ordering::SeqCst);
            capture::link().talk();
        }
    });
}

/// Ctrl Alt N was let go: what was said while it was held is kept.
fn hotkey_released() {
    if HOTKEY_DOWN.swap(false, Ordering::SeqCst) && TALKING.swap(false, Ordering::SeqCst) {
        capture::link().let_go();
    }
}

/// In a development build with `DIRT_ACTIONS_FROM_STDIN` set, each line
/// read from the standard input does what a hand would do outside the
/// window: `hotkey` presses Ctrl Alt N and lets it go, `hotkey down` and
/// `hotkey up` press it and let it go apart (a hold talks), and
/// `new note`, `open dirt` and `quit` choose that in the tray's menu. They go the same way as the real
/// key and menu from there. That is for trying the app on a computer whose
/// keyboard and tray belong to someone else. A release build ignores it.
fn take_actions_from_stdin() {
    if settings::test_hook("DIRT_ACTIONS_FROM_STDIN").is_none() {
        return;
    }
    thread::spawn(|| {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            match line.trim() {
                "hotkey" => {
                    hotkey_pressed();
                    hotkey_released();
                }
                "hotkey down" => hotkey_pressed(),
                "hotkey up" => hotkey_released(),
                "new note" => tray::chosen(tray::Choice::NewNote),
                "open dirt" => tray::chosen(tray::Choice::OpenDirt),
                "quit" => tray::chosen(tray::Choice::Quit),
                other => tracing::warn!("Not an action: {other:?}"),
            }
        }
    });
}

fn initialize_hotkey_manager() -> Result<HotkeyManager, String> {
    const HOTKEY_RETRY_ATTEMPTS: usize = 6;
    const HOTKEY_RETRY_DELAY_MS: u64 = 300;

    for attempt in 1..=HOTKEY_RETRY_ATTEMPTS {
        match HotkeyManager::new() {
            Ok(manager) => {
                GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
                    tracing::debug!("GlobalHotKeyEvent received: state={:?}", event.state);
                    match event.state {
                        HotKeyState::Pressed => hotkey_pressed(),
                        HotKeyState::Released => hotkey_released(),
                    }
                }));
                return Ok(manager);
            }
            Err(error) => {
                if attempt == HOTKEY_RETRY_ATTEMPTS {
                    return Err(error.to_string());
                }
                tracing::warn!(
                    "Hotkey registration attempt {attempt}/{HOTKEY_RETRY_ATTEMPTS} failed: {}. Retrying...",
                    error
                );
                thread::sleep(Duration::from_millis(HOTKEY_RETRY_DELAY_MS));
            }
        }
    }

    Err("Hotkey registration failed".to_string())
}
