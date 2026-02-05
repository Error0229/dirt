//! Main application component

use std::sync::atomic::Ordering;
use std::time::Duration;

use dioxus::desktop::window;
use dioxus::prelude::*;

use crate::components::{open_quick_capture_window, SettingsPanel};
use crate::services::DatabaseService;
use crate::state::AppState;
use crate::theme::{resolve_theme, ResolvedTheme};
use crate::tray::{process_tray_events, QUIT_REQUESTED, SHOW_MAIN_WINDOW};
use crate::views::Home;
use crate::{HOTKEY_TRIGGERED, TRAY_ENABLED};

/// Root application component
#[component]
pub fn App() -> Element {
    // State signals
    let mut notes = use_signal(Vec::new);
    let current_note_id = use_signal(|| None);
    let search_query = use_signal(String::new);
    let active_tag_filter = use_signal(|| None::<String>);
    let mut settings = use_signal(dirt_core::models::Settings::default);
    let mut theme = use_signal(|| resolve_theme(dirt_core::models::ThemeMode::System));
    let settings_open = use_signal(|| false);
    let mut db_service: Signal<Option<DatabaseService>> = use_signal(|| None);
    let mut db_initialized = use_signal(|| false);

    // Initialize database asynchronously (only once)
    use_effect(move || {
        if db_initialized() {
            return;
        }
        db_initialized.set(true); // Mark immediately to prevent double init

        spawn(async move {
            match DatabaseService::new().await {
                Ok(db) => {
                    // Load initial settings
                    let loaded_settings = db.load_settings().await.unwrap_or_default();
                    let resolved_theme = resolve_theme(loaded_settings.theme);

                    // Load initial notes
                    let loaded_notes = db.list_notes(100, 0).await.unwrap_or_default();
                    tracing::info!("Loaded {} notes from database", loaded_notes.len());

                    // Update state
                    settings.set(loaded_settings);
                    theme.set(resolved_theme);
                    notes.set(loaded_notes);
                    db_service.set(Some(db));
                }
                Err(e) => {
                    tracing::error!("Failed to initialize database: {}", e);
                }
            }
        });
    });

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

    use_context_provider(|| AppState {
        notes,
        current_note_id,
        search_query,
        active_tag_filter,
        settings,
        theme,
        db_service,
        settings_open,
    });

    let current_theme = theme();
    let colors = current_theme.palette();
    let current_settings = settings();
    let theme_attr = match current_theme {
        ResolvedTheme::Light => "light",
        ResolvedTheme::Dark => "dark",
    };

    rsx! {
        // Load theme CSS for Dioxus components
        document::Link { rel: "stylesheet", href: asset!("/assets/dx-components-theme.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/theme-overrides.css") }

        div {
            class: "app-container",
            "data-theme": "{theme_attr}",
            style: "
                min-height: 100vh;
                font-family: {current_settings.font_family}, system-ui, -apple-system, sans-serif;
                font-size: {current_settings.font_size}px;
                background: {colors.bg_primary};
                color: {colors.text_primary};
            ",
            Home {}

            // Settings panel overlay
            if settings_open() {
                SettingsPanel {}
            }
        }
    }
}
