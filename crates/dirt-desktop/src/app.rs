//! Main application component.
//!
//! Sync runs in the background — there is no manual "Sync now" button.
//! The worker (see `services::sync_worker`) handles startup-pull,
//! 30 s periodic, and post-mutation kicks; mutation sites call
//! `AppState::trigger_sync()` after a successful DB write.
//!
//! `DIRT_API_BASE_URL` and `DIRT_CLIENT_TOKEN` must be set when the
//! app launches. If either is missing or invalid, we surface a loud
//! `SyncStatus::Error` instead of silently running offline.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::{window, LogicalPosition, LogicalSize};
use dioxus::prelude::*;
use dirt_core::models::Note;
use dirt_core::sync::api_client::ApiClient;

use crate::bootstrap_config::{load_bootstrap_config, resolve_bootstrap_config, BootstrapConfig};
use crate::components::{QuickCapture, SettingsPanel};
use crate::queries::use_notes_query;
use crate::services::{
    spawn_sync_worker, DatabaseService, SyncEvent, SyncWorkerHandle, TranscriptionService,
};
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
    let note_list_visible = use_signal(|| true);
    let mut saved_window_geometry: Signal<Option<(f64, f64, f64, f64)>> = use_signal(|| None);
    let mut db_service: Signal<Option<Arc<DatabaseService>>> = use_signal(|| None);
    let mut sync_worker: Signal<Option<SyncWorkerHandle>> = use_signal(|| None);
    let mut sync_worker_started = use_signal(|| false);
    let transcription_service: Signal<Option<Arc<TranscriptionService>>> =
        use_signal(|| match TranscriptionService::new() {
            Ok(service) => Some(Arc::new(service)),
            Err(error) => {
                tracing::warn!("Voice transcription service unavailable: {}", error);
                None
            }
        });
    let mut bootstrap_ready = use_signal(|| false);
    let mut bootstrap_config: Signal<Option<BootstrapConfig>> = use_signal(|| None);
    let sync_status = use_signal(|| SyncStatus::Offline);
    let sync_issue = use_signal(|| None::<String>);
    let last_sync_at = use_signal(|| None::<i64>);
    let pending_sync_count = use_signal(|| 0usize);
    let pending_sync_note_ids = use_signal(Vec::new);
    let embedded_bootstrap_config = load_bootstrap_config();

    // Resolve runtime bootstrap manifest. The auto-sync worker reads
    // the resolved config (or the embedded fallback) below.
    use_effect(move || {
        if bootstrap_ready() {
            return;
        }
        let fallback_bootstrap = embedded_bootstrap_config.clone();

        spawn(async move {
            let resolved = match resolve_bootstrap_config(fallback_bootstrap.clone()).await {
                Ok(config) => config,
                Err(error) => {
                    tracing::warn!(
                        "Failed to resolve runtime bootstrap manifest ({}). Falling back to embedded desktop bootstrap values.",
                        error
                    );
                    fallback_bootstrap
                }
            };
            bootstrap_config.set(Some(resolved));
            bootstrap_ready.set(true);
        });
    });

    let mut sync_status_signal = sync_status;
    let mut sync_issue_signal = sync_issue;
    let mut last_sync_at_signal = last_sync_at;

    // Initialize the local database, then spawn the auto-sync worker
    // exactly once. The worker is misconfigured-loud: a missing
    // DIRT_API_BASE_URL or DIRT_CLIENT_TOKEN leaves `sync_status` in
    // `Error` and `sync_issue` populated so the user notices.
    //
    // Status flows worker → UI via an unbounded mpsc channel and a
    // dioxus `use_future` consumer (see below). Dioxus signals can't
    // be captured into a Send closure on a tokio task, so the channel
    // is the bridge.
    let _db_init_task = use_resource(move || async move {
        if !bootstrap_ready() {
            return;
        }

        match DatabaseService::new().await {
            Ok(db) => {
                let db = Arc::new(db);

                let loaded_settings = match db.load_settings_with_large_stack().await {
                    Ok(settings) => settings,
                    Err(error) => {
                        tracing::error!("Failed to load desktop settings: {error}");
                        db_service.set(None);
                        return;
                    }
                };
                let resolved_theme = resolve_theme(loaded_settings.theme);
                settings.set(loaded_settings);
                theme.set(resolved_theme);

                db_service.set(Some(db.clone()));

                if !sync_worker_started() {
                    sync_worker_started.set(true);
                    let bootstrap = bootstrap_config().unwrap_or_default();
                    match build_api_client(&bootstrap) {
                        Ok(api) => {
                            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SyncEvent>();
                            let handle = spawn_sync_worker(db, Arc::new(api), tx);
                            sync_worker.set(Some(handle));
                            // Drain status events into UI signals.
                            spawn(async move {
                                while let Some(event) = rx.recv().await {
                                    match event {
                                        SyncEvent::Status(status) => {
                                            sync_status_signal.set(status);
                                        }
                                        SyncEvent::Issue(issue) => {
                                            sync_issue_signal.set(issue);
                                        }
                                        SyncEvent::LastSync(ts) => {
                                            last_sync_at_signal.set(Some(ts));
                                        }
                                    }
                                }
                            });
                            tracing::info!("Sync worker spawned");
                        }
                        Err(error) => {
                            tracing::error!("Sync worker not started: {error}");
                            sync_status_signal.set(SyncStatus::Error);
                            sync_issue_signal.set(Some(error));
                        }
                    }
                }
            }
            Err(error) => {
                tracing::error!("Failed to initialize database: {error}");
                db_service.set(None);
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
        transcription_service,
        sync_worker,
        sync_status,
        sync_issue,
        last_sync_at,
        pending_sync_count,
        pending_sync_note_ids,
        settings_open,
        quick_capture_open,
        note_list_visible,
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

/// Resolve API base URL + client token at app launch.
///
/// `DIRT_API_BASE_URL` falls back to the build-baked bootstrap value
/// (so a packaged binary works without env config), but
/// `DIRT_CLIENT_TOKEN` is runtime-only — it must never get baked into a
/// binary that ships to users. A missing or empty token is a hard
/// error: the worker won't start, the UI shows `Error`, and the user
/// has to fix their environment rather than the app silently running
/// without sync.
fn build_api_client(bootstrap: &BootstrapConfig) -> Result<ApiClient, String> {
    let base_url = std::env::var("DIRT_API_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| bootstrap.dirt_api_base_url.clone())
        .ok_or_else(|| {
            "DIRT_API_BASE_URL is not configured (set it in the environment or in .env.client at build time).".to_string()
        })?;

    let token = std::env::var("DIRT_CLIENT_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "DIRT_CLIENT_TOKEN is not set in the environment — sync cannot run without a bearer token.".to_string()
        })?;

    ApiClient::new(base_url, token).map_err(|err| format!("invalid sync configuration: {err}"))
}
