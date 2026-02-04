//! Dirt Desktop Application
//!
//! A cross-platform desktop app for capturing fleeting thoughts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod components;
mod hotkey;
mod services;
mod state;
mod theme;
mod views;

use std::sync::atomic::{AtomicBool, Ordering};

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use hotkey::HotkeyManager;

/// Atomic flag for hotkey events - shared between event handler and UI
pub static HOTKEY_TRIGGERED: AtomicBool = AtomicBool::new(false);

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Dirt...");

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

    dioxus::launch(app::App);
}
