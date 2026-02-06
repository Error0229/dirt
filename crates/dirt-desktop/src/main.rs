//! Dirt Desktop Application
//!
//! A cross-platform desktop app for capturing fleeting thoughts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod components;
mod hotkey;
mod queries;
mod services;
mod state;
mod theme;
mod tray;
mod views;

use std::sync::atomic::{AtomicBool, Ordering};

use dioxus::desktop::{Config, WindowCloseBehaviour};
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use hotkey::HotkeyManager;
use tray::TrayManager;

/// Atomic flag for hotkey events - shared between event handler and UI
pub static HOTKEY_TRIGGERED: AtomicBool = AtomicBool::new(false);

/// Atomic flag indicating tray is enabled
pub static TRAY_ENABLED: AtomicBool = AtomicBool::new(false);

#[allow(clippy::cognitive_complexity)]
fn main() {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Dirt...");

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

    // Initialize global hotkey BEFORE launching Dioxus
    // The manager must be kept alive and stay on the main thread
    let _hotkey_manager = match HotkeyManager::new() {
        Ok(manager) => {
            // Set up event handler that sets the atomic flag
            GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
                tracing::debug!("GlobalHotKeyEvent received: state={:?}", event.state);
                if event.state == HotKeyState::Pressed {
                    tracing::info!("Hotkey pressed (Ctrl+Alt+N) - setting flag");
                    HOTKEY_TRIGGERED.store(true, Ordering::SeqCst);
                }
            }));
            Some(manager)
        }
        Err(e) => {
            tracing::error!("Failed to register hotkey: {}", e);
            None
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
