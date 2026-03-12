//! Dirt Desktop Application
//!
//! A cross-platform desktop app for capturing fleeting thoughts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Dioxus `asset!` macro expansion triggers false positives for
// `clippy::volatile_composites` on newer toolchains.
#![allow(unknown_lints, clippy::volatile_composites)]

mod app;
mod bootstrap_config;
mod components;
mod hotkey;
mod queries;
mod services;
mod state;
mod theme;
mod tray;
mod views;

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use dioxus::desktop::{Config, WindowCloseBehaviour};
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use hotkey::HotkeyManager;
use single_instance::SingleInstance;
use tray::TrayManager;

/// Atomic flag for hotkey events - shared between event handler and UI
pub static HOTKEY_TRIGGERED: AtomicBool = AtomicBool::new(false);

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
    let _tray_manager = match TrayManager::new() {
        Ok(manager) => {
            tracing::info!("System tray initialized");
            TRAY_ENABLED.store(true, Ordering::SeqCst);
            Some(manager)
        }
        Err(e) => {
            tracing::error!("Failed to initialize system tray: {}", e);
            None
        }
    };

    // Initialize global hotkey BEFORE launching Dioxus.
    // The manager must be kept alive and stay on the main thread.
    // Retry briefly to tolerate OS-level release delay from a previous instance.
    let _hotkey_manager = match initialize_hotkey_manager() {
        Ok(manager) => Some(manager),
        Err(e) => {
            tracing::error!("Failed to register hotkey after retries: {}", e);
            tracing::error!(
                "Desktop instance startup aborted to avoid multi-instance sync conflicts."
            );
            return;
        }
    };

    // Configure Dioxus to hide window on close instead of exiting
    // Hide window instead of exiting when closed - keeps app running in tray
    let config = Config::new().with_close_behaviour(WindowCloseBehaviour::WindowHides);

    // Launch the app
    dioxus::LaunchBuilder::new()
        .with_cfg(config)
        .launch(app::App);
}

fn initialize_hotkey_manager() -> Result<HotkeyManager, String> {
    const HOTKEY_RETRY_ATTEMPTS: usize = 6;
    const HOTKEY_RETRY_DELAY_MS: u64 = 300;

    for attempt in 1..=HOTKEY_RETRY_ATTEMPTS {
        match HotkeyManager::new() {
            Ok(manager) => {
                GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
                    tracing::debug!("GlobalHotKeyEvent received: state={:?}", event.state);
                    if event.state == HotKeyState::Pressed {
                        tracing::info!("Hotkey pressed (Ctrl+Alt+N) - setting flag");
                        HOTKEY_TRIGGERED.store(true, Ordering::SeqCst);
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
