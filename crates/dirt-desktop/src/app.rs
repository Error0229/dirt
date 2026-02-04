//! Main application component

use std::sync::atomic::Ordering;
use std::time::Duration;

use dioxus::desktop::window;
use dioxus::prelude::*;

use crate::components::open_quick_capture_window;
use crate::services::DatabaseService;
use crate::state::AppState;
use crate::theme::Theme;
use crate::tray::{process_tray_events, QUIT_REQUESTED, SHOW_MAIN_WINDOW};
use crate::views::Home;
use crate::{HOTKEY_TRIGGERED, TRAY_ENABLED};

/// Root application component
#[component]
pub fn App() -> Element {
    // Initialize database service
    let db_service = use_signal(|| {
        DatabaseService::new()
            .map_err(|e| tracing::error!("Failed to initialize database: {}", e))
            .ok()
    });

    // Initialize global state
    let mut notes = use_signal(Vec::new);
    let current_note_id = use_signal(|| None);
    let search_query = use_signal(String::new);
    let active_tag_filter = use_signal(|| None::<String>);
    let theme = use_signal(Theme::default);

    // Poll for hotkey and tray events
    use_future(move || async move {
        let tray_enabled = TRAY_ENABLED.load(Ordering::SeqCst);
        loop {
            // Process tray menu events
            if tray_enabled {
                process_tray_events();

                // Check for show window request
                if SHOW_MAIN_WINDOW.swap(false, Ordering::SeqCst) {
                    tracing::info!("Showing main window from tray");
                    window().set_visible(true);
                    window().set_focus();
                }

                // Check for quit request
                if QUIT_REQUESTED.swap(false, Ordering::SeqCst) {
                    tracing::info!("Quit requested from tray");
                    std::process::exit(0);
                }
            }

            // Check if hotkey was triggered
            if HOTKEY_TRIGGERED.swap(false, Ordering::SeqCst) {
                tracing::info!("Opening quick capture window");
                open_quick_capture_window();
            }

            // Poll at ~60fps
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    });

    // Load notes from database on startup
    use_effect(move || {
        if let Some(ref db) = *db_service.read() {
            match db.list_notes(100, 0) {
                Ok(loaded_notes) => {
                    tracing::info!("Loaded {} notes from database", loaded_notes.len());
                    notes.set(loaded_notes);
                }
                Err(e) => {
                    tracing::error!("Failed to load notes: {}", e);
                }
            }
        }
    });

    use_context_provider(|| AppState {
        notes,
        current_note_id,
        search_query,
        active_tag_filter,
        theme,
        db_service,
    });

    let theme_class = if theme().is_dark() { "dark" } else { "light" };

    rsx! {
        div {
            class: "app-container {theme_class}",
            style: "min-height: 100vh; font-family: system-ui, -apple-system, sans-serif;",
            Home {}
        }
    }
}
