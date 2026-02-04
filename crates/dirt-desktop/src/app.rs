//! Main application component

use std::time::Duration;

use dioxus::prelude::*;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};

use crate::components::QuickCapture;
use crate::hotkey::HotkeyManager;
use crate::services::DatabaseService;
use crate::state::AppState;
use crate::theme::Theme;
use crate::views::Home;

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
    let mut show_quick_capture = use_signal(|| false);

    // Initialize hotkey manager (kept alive for the app lifetime)
    let _hotkey_manager = use_signal(|| {
        HotkeyManager::new()
            .map_err(|e| tracing::error!("Failed to register hotkey: {}", e))
            .ok()
    });

    // Listen for hotkey events
    use_effect(move || {
        spawn(async move {
            let receiver = GlobalHotKeyEvent::receiver();
            loop {
                if let Ok(event) = receiver.try_recv() {
                    if event.state == HotKeyState::Pressed {
                        tracing::debug!("Hotkey pressed, toggling quick capture");
                        let current = *show_quick_capture.read();
                        show_quick_capture.set(!current);
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
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
        show_quick_capture,
    });

    let theme_class = if theme().is_dark() { "dark" } else { "light" };

    rsx! {
        div {
            class: "app-container {theme_class}",
            style: "min-height: 100vh; font-family: system-ui, -apple-system, sans-serif;",
            Home {}

            if show_quick_capture() {
                QuickCapture {
                    on_close: move |()| show_quick_capture.set(false)
                }
            }
        }
    }
}
