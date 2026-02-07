//! Main application component

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::{window, LogicalPosition, LogicalSize};
use dioxus::prelude::*;
use dirt_core::models::Note;

use crate::components::{QuickCapture, SettingsPanel};
use crate::queries::use_notes_query;
use crate::services::{AuthSession, DatabaseService, SupabaseAuthService};
use crate::state::{AppState, SyncStatus};
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
    let mut quick_capture_open = use_signal(|| false);
    let mut saved_window_geometry: Signal<Option<(f64, f64, f64, f64)>> = use_signal(|| None);
    let mut db_service: Signal<Option<Arc<DatabaseService>>> = use_signal(|| None);
    let mut auth_service: Signal<Option<Arc<SupabaseAuthService>>> = use_signal(|| None);
    let mut auth_session: Signal<Option<AuthSession>> = use_signal(|| None);
    let mut auth_error: Signal<Option<String>> = use_signal(|| None);
    let mut db_initialized = use_signal(|| false);
    let mut auth_initialized = use_signal(|| false);
    let mut sync_status = use_signal(|| SyncStatus::Offline);
    let mut last_sync_at = use_signal(|| None::<i64>);
    let mut pending_sync_count = use_signal(|| 0usize);
    let mut pending_sync_note_ids = use_signal(Vec::new);

    // Initialize authentication service and restore persisted session.
    use_effect(move || {
        if auth_initialized() {
            return;
        }
        auth_initialized.set(true);

        spawn(async move {
            match SupabaseAuthService::new_from_env() {
                Ok(Some(service)) => {
                    let service = Arc::new(service);
                    match service.restore_session().await {
                        Ok(session) => {
                            auth_session.set(session);
                            auth_error.set(None);
                        }
                        Err(error) => {
                            tracing::error!("Failed to restore auth session: {}", error);
                            auth_error.set(Some(error.to_string()));
                        }
                    }
                    auth_service.set(Some(service));
                }
                Ok(None) => {
                    auth_service.set(None);
                }
                Err(error) => {
                    tracing::error!("Failed to initialize auth service: {}", error);
                    auth_service.set(None);
                    auth_error.set(Some(error.to_string()));
                }
            }
        });
    });

    // Initialize database asynchronously (only once)
    use_effect(move || {
        if db_initialized() {
            return;
        }
        db_initialized.set(true);

        spawn(async move {
            match DatabaseService::new().await {
                Ok(db) => {
                    let db = Arc::new(db);

                    // Load initial settings
                    let loaded_settings = db.load_settings().await.unwrap_or_default();
                    let resolved_theme = resolve_theme(loaded_settings.theme);

                    // Update state
                    settings.set(loaded_settings);
                    theme.set(resolved_theme);

                    if db.is_sync_enabled().await {
                        sync_status.set(SyncStatus::Syncing);
                        match db.sync().await {
                            Ok(()) => {
                                sync_status.set(SyncStatus::Synced);
                                last_sync_at.set(Some(chrono::Utc::now().timestamp_millis()));
                                pending_sync_count.set(0);
                                pending_sync_note_ids.write().clear();
                            }
                            Err(error) => {
                                tracing::error!("Initial sync failed: {}", error);
                                sync_status.set(SyncStatus::Error);
                            }
                        }
                    } else {
                        sync_status.set(SyncStatus::Offline);
                    }

                    db_service.set(Some(db));
                }
                Err(e) => {
                    tracing::error!("Failed to initialize database: {}", e);
                }
            }
        });
    });

    // Periodically sync and update sync status metadata.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;

            let db = db_service.read().clone();
            let Some(db) = db else {
                sync_status.set(SyncStatus::Offline);
                continue;
            };

            if !db.is_sync_enabled().await {
                sync_status.set(SyncStatus::Offline);
                continue;
            }

            sync_status.set(SyncStatus::Syncing);
            match db.sync().await {
                Ok(()) => {
                    sync_status.set(SyncStatus::Synced);
                    last_sync_at.set(Some(chrono::Utc::now().timestamp_millis()));
                    pending_sync_count.set(0);
                    pending_sync_note_ids.write().clear();
                }
                Err(error) => {
                    tracing::error!("Periodic sync failed: {}", error);
                    sync_status.set(SyncStatus::Error);
                }
            }
        }
    });

    // Use dioxus-query for reactive notes fetching (called unconditionally - rules of hooks)
    let notes_query = use_notes_query(db_service.read().clone());

    // Poll for hotkey, tray events, and sync query results to notes signal
    use_future(move || async move {
        let tray_enabled = TRAY_ENABLED.load(Ordering::SeqCst);
        // Track last query result to detect when the *query* produces new data,
        // without clobbering optimistic updates in the notes signal.
        let mut last_query_result: Option<Vec<Note>> = None;
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
                    std::process::exit(0);
                }
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

            // Sync query result to notes signal only when the query itself changes.
            // Comparing against last_query_result (not notes signal) avoids clobbering
            // optimistic updates that temporarily diverge from the query.
            {
                let query_reader = notes_query.read();
                let fetched = query_reader.state().ok().cloned();
                drop(query_reader);
                if let Some(fetched_notes) = fetched {
                    let changed = last_query_result
                        .as_ref()
                        .map_or(true, |prev| *prev != fetched_notes);
                    if changed {
                        tracing::debug!("Notes query returned {} notes", fetched_notes.len());
                        last_query_result = Some(fetched_notes.clone());
                        notes.set(fetched_notes);
                    }
                }
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
        auth_service,
        auth_session,
        auth_error,
        sync_status,
        last_sync_at,
        pending_sync_count,
        pending_sync_note_ids,
        settings_open,
        quick_capture_open,
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
        document::Link {
            rel: "stylesheet",
            href: asset!("/assets/dx-components-theme.css"),
        }
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
            if quick_capture_open() {
                QuickCapture {}
            } else {
                Home {}

                if settings_open() {
                    SettingsPanel {}
                }
            }
        }
    }
}
