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

use std::thread;

use dioxus::desktop::{Config, HotKeyState, WindowCloseBehaviour};
use dioxus::prelude::*;
use single_instance::SingleInstance;

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

    // Copies of files handed to other programs in earlier runs.
    services::clear_opened_copies(&services::data_folder());

    // Closing the window hides it, and Dirt stays in the tray (`tray`).
    // A click on the tray icon does nothing: Dioxus would show every
    // window, quick capture's too. A double click opens Dirt.
    let config = without_the_web_views_own_keys(
        Config::new()
            .with_window(screen::window::builder())
            .with_close_behaviour(WindowCloseBehaviour::WindowHides)
            .with_tray_icon_show_window_on_click(false),
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

/// Stand-ins on the standard input for the hotkey and the tray's menu.
///
/// In a development build with `DIRT_ACTIONS_FROM_STDIN` set, each line
/// read from the standard input does what a hand would do outside the
/// window: `hotkey` presses Ctrl Alt N and lets it go, `hotkey down` and
/// `hotkey up` press it and let it go apart (a hold talks), and
/// `new note`, `open dirt` and `quit` choose that in the tray's menu. They
/// go the same way as the real key and menu from there. That is for trying
/// the app on a computer whose keyboard and tray belong to someone else. A
/// release build ignores it.
pub fn use_actions_from_stdin(hotkey: Callback<HotKeyState>, choose: Callback<tray::Choice>) {
    use_future(move || async move {
        if settings::test_hook("DIRT_ACTIONS_FROM_STDIN").is_none() {
            return;
        }
        let (send, mut lines) = tokio::sync::mpsc::unbounded_channel();
        thread::spawn(move || {
            for line in std::io::stdin().lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        while let Some(line) = lines.recv().await {
            match line.trim() {
                "hotkey" => {
                    hotkey.call(HotKeyState::Pressed);
                    hotkey.call(HotKeyState::Released);
                }
                "hotkey down" => hotkey.call(HotKeyState::Pressed),
                "hotkey up" => hotkey.call(HotKeyState::Released),
                "new note" => choose.call(tray::Choice::NewNote),
                "open dirt" => choose.call(tray::Choice::OpenDirt),
                "quit" => choose.call(tray::Choice::Quit),
                other => tracing::warn!("Not an action: {other:?}"),
            }
        }
    });
}
