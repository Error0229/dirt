//! Main application component

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::{window, LogicalPosition, LogicalSize};
use dioxus::prelude::*;
use dirt_core::db::SyncConfig;
use dirt_core::models::Note;

use crate::bootstrap_config::{load_bootstrap_config, resolve_bootstrap_config};
use crate::components::{QuickCapture, SettingsPanel};
use crate::queries::use_notes_query;
use crate::services::{
    auth_service_from_bootstrap, media_client_from_bootstrap, sync_auth_from_bootstrap,
    AuthSession, DatabaseService, DesktopAuthService, MediaApiClient, TranscriptionService,
    TursoSyncAuthClient,
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
    let mut auth_service: Signal<Option<Arc<DesktopAuthService>>> = use_signal(|| None);
    let mut sync_auth_client: Signal<Option<Arc<TursoSyncAuthClient>>> = use_signal(|| None);
    let mut media_api_client: Signal<Option<Arc<MediaApiClient>>> = use_signal(|| None);
    let transcription_service: Signal<Option<Arc<TranscriptionService>>> =
        use_signal(|| match TranscriptionService::new() {
            Ok(service) => Some(Arc::new(service)),
            Err(error) => {
                tracing::warn!("Voice transcription service unavailable: {}", error);
                None
            }
        });
    let mut auth_session: Signal<Option<AuthSession>> = use_signal(|| None);
    let mut auth_error: Signal<Option<String>> = use_signal(|| None);
    let mut db_reconnect_version = use_signal(|| 0u64);
    let mut auth_initialized = use_signal(|| false);
    let mut bootstrap_ready = use_signal(|| false);
    let mut sync_status = use_signal(|| SyncStatus::Offline);
    let mut sync_issue = use_signal(|| None::<String>);
    let mut last_sync_at = use_signal(|| None::<i64>);
    let mut pending_sync_count = use_signal(|| 0usize);
    let mut pending_sync_note_ids = use_signal(Vec::new);
    let mut sync_token_expires_at = use_signal(|| None::<i64>);
    let embedded_bootstrap_config = load_bootstrap_config();

    // Initialize authentication service and restore persisted session.
    use_effect(move || {
        if auth_initialized() {
            return;
        }
        auth_initialized.set(true);
        let fallback_bootstrap = embedded_bootstrap_config.clone();

        spawn(async move {
            let bootstrap = match resolve_bootstrap_config(fallback_bootstrap.clone()).await {
                Ok(config) => config,
                Err(error) => {
                    tracing::warn!(
                        "Failed to resolve runtime bootstrap manifest ({}). Falling back to embedded desktop bootstrap values.",
                        error
                    );
                    fallback_bootstrap
                }
            };

            match sync_auth_from_bootstrap(&bootstrap) {
                Ok(Some(client)) => sync_auth_client.set(Some(Arc::new(client))),
                Ok(None) => sync_auth_client.set(None),
                Err(error) => {
                    tracing::warn!("Managed sync auth bootstrap is invalid: {}", error);
                    sync_auth_client.set(None);
                }
            }

            match media_client_from_bootstrap(&bootstrap) {
                Ok(Some(client)) => media_api_client.set(Some(Arc::new(client))),
                Ok(None) => media_api_client.set(None),
                Err(error) => {
                    tracing::warn!("Managed media bootstrap is invalid: {}", error);
                    media_api_client.set(None);
                }
            }

            let service_result = auth_service_from_bootstrap(&bootstrap);

            match service_result {
                Ok(Some(service)) => {
                    let service = Arc::new(service);
                    match service.restore_session().await {
                        Ok(session) => {
                            auth_session.set(session);
                            auth_error.set(None);
                            db_reconnect_version.set(db_reconnect_version().saturating_add(1));
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

            bootstrap_ready.set(true);
        });
    });

    // Initialize or reconnect database when auth/session context changes.
    // `use_resource` reruns when read signals change.
    let _db_init_task = use_resource(move || async move {
        let _db_reconnect_version = db_reconnect_version();
        if !bootstrap_ready() {
            return;
        }

        let current_session = auth_session.peek().clone();
        let managed_sync_client = sync_auth_client.peek().clone();
        let managed_sync_expected = managed_sync_client.is_some() && current_session.is_some();
        let had_existing_db = db_service.peek().is_some();

        // Force-drop the previous local connection before opening a sync replica.
        // libsql remote replicas can fail to initialize when the same db file is still held.
        if managed_sync_expected && had_existing_db {
            db_service.set(None);
            tokio::time::sleep(Duration::from_millis(120)).await;
        }

        if !managed_sync_expected {
            sync_issue.set(None);
        }

        let db_result =
            if let (Some(client), Some(mut session)) = (managed_sync_client, current_session) {
                // Refresh the Supabase access token if it has expired,
                // otherwise `exchange_token` will be rejected with 401.
                if session.is_expired() {
                    if let Some(service) = auth_service.peek().as_ref() {
                        match service.refresh_session(&session.refresh_token).await {
                            Ok(refreshed) => {
                                tracing::info!(
                                    "Supabase session refreshed, new expires_at={}",
                                    refreshed.expires_at
                                );
                                auth_session.set(Some(refreshed.clone()));
                                session = refreshed;
                            }
                            Err(error) => {
                                let message = format!("Supabase session refresh failed: {error}");
                                tracing::error!("{message}");
                                sync_issue.set(Some(message));
                                return;
                            }
                        }
                    } else {
                        let message =
                            "Supabase session expired but auth service is unavailable".to_string();
                        tracing::error!("{message}");
                        sync_issue.set(Some(message));
                        return;
                    }
                }

                match client.exchange_token(&session.access_token).await {
                    Ok(token) => {
                        sync_token_expires_at.set(Some(token.expires_at));
                        sync_issue.set(None);
                        let sync_config = SyncConfig::new(token.database_url, token.token);
                        DatabaseService::new_with_sync(sync_config).await
                    }
                    Err(error) => {
                        sync_token_expires_at.set(None);
                        let message = format!("Managed sync token exchange failed: {error}");
                        sync_issue.set(Some(message.clone()));
                        Err(dirt_core::Error::Storage(message))
                    }
                }
            } else {
                sync_token_expires_at.set(None);
                DatabaseService::new().await
            };

        match db_result {
            Ok(db) => {
                let db = Arc::new(db);

                let loaded_settings = match db.load_settings_with_large_stack().await {
                    Ok(settings) => settings,
                    Err(error) => {
                        let message = format!("Failed to load desktop settings: {error}");
                        tracing::error!("{message}");
                        sync_issue.set(Some(message));
                        sync_status.set(SyncStatus::Error);
                        db_service.set(None);
                        return;
                    }
                };
                let resolved_theme = resolve_theme(loaded_settings.theme);
                settings.set(loaded_settings);
                theme.set(resolved_theme);

                if db.is_sync_enabled().await {
                    sync_status.set(SyncStatus::Syncing);
                    match db.sync_with_large_stack().await {
                        Ok(()) => {
                            sync_status.set(SyncStatus::Synced);
                            sync_issue.set(None);
                            last_sync_at.set(Some(chrono::Utc::now().timestamp_millis()));
                            pending_sync_count.set(0);
                            pending_sync_note_ids.write().clear();
                        }
                        Err(error) => {
                            let message = format!("Initial sync failed: {error}");
                            tracing::error!("{message}");
                            sync_issue.set(Some(message));
                            sync_status.set(SyncStatus::Error);
                        }
                    }
                } else if managed_sync_expected {
                    let message = "Signed in, but this database connection is running without managed sync credentials.".to_string();
                    tracing::error!("{message}");
                    sync_issue.set(Some(message));
                    sync_status.set(SyncStatus::Error);
                } else {
                    sync_issue.set(None);
                    sync_status.set(SyncStatus::Offline);
                }

                db_service.set(Some(db));
            }
            Err(error) => {
                let message = format!("Failed to initialize database: {error}");
                tracing::error!("{message}");
                sync_issue.set(Some(message));
                sync_status.set(SyncStatus::Error);
                db_service.set(None);
            }
        }
    });

    // Periodically sync and update sync status metadata.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let cloud_sync_expected = sync_auth_client.read().is_some() && auth_session().is_some();

            // Proactively refresh the sync token before it expires.
            // Turso tokens are short-lived (~15min) and libSQL bakes them
            // into the connection at construction time, so we must re-open
            // the database with fresh credentials before the token lapses.
            if let Some(expires_at) = sync_token_expires_at() {
                let now = chrono::Utc::now().timestamp();
                if now >= expires_at - 120 {
                    tracing::info!(
                        "Sync token expires in {}s, triggering credential refresh",
                        expires_at - now
                    );
                    db_reconnect_version.set(db_reconnect_version() + 1);
                    continue;
                }
            }

            let db = db_service.read().clone();
            let Some(db) = db else {
                if cloud_sync_expected {
                    sync_status.set(SyncStatus::Error);
                    if sync_issue().is_none() {
                        sync_issue.set(Some(
                            "Signed in, but sync database service is not initialized.".to_string(),
                        ));
                    }
                } else {
                    sync_issue.set(None);
                    sync_status.set(SyncStatus::Offline);
                }
                continue;
            };

            if !db.is_sync_enabled().await {
                if cloud_sync_expected {
                    sync_status.set(SyncStatus::Error);
                    if sync_issue().is_none() {
                        sync_issue.set(Some(
                            "Cloud sync is expected for this session, but the database is running local-only."
                                .to_string(),
                        ));
                    }
                } else {
                    sync_issue.set(None);
                    sync_status.set(SyncStatus::Offline);
                }
                continue;
            }

            sync_status.set(SyncStatus::Syncing);
            match db.sync_with_large_stack().await {
                Ok(()) => {
                    sync_status.set(SyncStatus::Synced);
                    sync_issue.set(None);
                    last_sync_at.set(Some(chrono::Utc::now().timestamp_millis()));
                    pending_sync_count.set(0);
                    pending_sync_note_ids.write().clear();
                }
                Err(error) => {
                    let message = format!("{error}");

                    // Detect expired/revoked tokens and trigger credential refresh
                    // instead of staying stuck in an error loop.
                    if message.contains("401")
                        || message.contains("Unauthorized")
                        || message.contains("token expired")
                    {
                        tracing::warn!(
                            "Sync rejected with auth error, triggering credential refresh: {message}"
                        );
                        db_reconnect_version.set(db_reconnect_version() + 1);
                        continue;
                    }

                    // Detect corrupted local replica and trigger database
                    // reconnection so the startup recovery can quarantine the
                    // bad file and pull a fresh copy from Turso.
                    let lower = message.to_ascii_lowercase();
                    if lower.contains("file is not a database")
                        || lower.contains("wal frame insert conflict")
                    {
                        tracing::warn!(
                            "Sync detected corrupted local replica, triggering reconnection: {message}"
                        );
                        db_reconnect_version.set(db_reconnect_version() + 1);
                        continue;
                    }

                    let message = format!("Periodic sync failed: {error}");
                    tracing::error!("{message}");
                    sync_issue.set(Some(message));
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
        media_api_client,
        transcription_service,
        auth_session,
        auth_error,
        db_reconnect_version,
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
