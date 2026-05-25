//! Main application component.
//!
//! Sync runs in the background — there is no manual "Sync now" button.
//! The worker (see `services::sync_worker`) handles startup-pull,
//! 30 s periodic, and post-mutation kicks; mutation sites call
//! `AppState::trigger_sync()` after a successful DB write.
//!
//! The session bearer lives in the OS-native keyring
//! ([`KeyringTokenStore`]), not in an environment variable. When the
//! keyring is empty, sync stays Offline and the worker doesn't spawn —
//! sync is an opt-in feature surfaced through Settings → Account.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::{window, LogicalPosition, LogicalSize};
use dioxus::prelude::*;
use dirt_core::auth::{AuthClient, KeyringTokenStore};
use dirt_core::models::Note;
use dirt_core::sync::session_client::SessionApiClient;

use crate::bootstrap_config::{load_bootstrap_config, BootstrapConfig};
use crate::components::{QuickCapture, SettingsPanel};
use crate::queries::use_notes_query;
use crate::services::{
    spawn_sync_worker, DatabaseService, SyncEvent, SyncWorkerHandle, TranscriptionService,
};
use crate::state::{AppState, AuthDeps, SyncStatus};
use crate::theme::{resolve_theme, ResolvedTheme};
use crate::tray::{process_tray_events, QUIT_REQUESTED, SHOW_MAIN_WINDOW};
use crate::views::Home;
use crate::{HOTKEY_TRIGGERED, TRAY_ENABLED};

/// Keyring service identifier for the desktop session token. Matches
/// the values used in [`dirt-cli`](../../../dirt-cli/src/commands/auth_cmd.rs)
/// so the two binaries share one keyring slot — logging in on the CLI
/// keeps you signed in on the desktop app and vice versa.
const KEYRING_SERVICE: &str = "dev.dirt.session";
/// Per-user discriminator inside the keyring service slot. Solo phase
/// uses a static `"default"`; multi-user / multi-account would pivot to
/// a user identifier.
const KEYRING_ACCOUNT: &str = "default";

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
    let sync_worker: Signal<Option<SyncWorkerHandle>> = use_signal(|| None);
    let mut sync_worker_started = use_signal(|| false);
    let mut signed_in = use_signal(|| None);
    let mut session_client: Signal<Option<Arc<SessionApiClient>>> = use_signal(|| None);
    let transcription_service: Signal<Option<Arc<TranscriptionService>>> =
        use_signal(|| match TranscriptionService::new() {
            Ok(service) => Some(Arc::new(service)),
            Err(error) => {
                tracing::warn!("Voice transcription service unavailable: {}", error);
                None
            }
        });
    // Bootstrap is build-baked (see `bootstrap_config::load_bootstrap_config`),
    // so it's available synchronously at component init. The Phase-1
    // managed-architecture's runtime `/v1/bootstrap` fetch was removed
    // when the server-side route was deleted; clients now read straight
    // from the embedded JSON.
    let bootstrap_config: Signal<BootstrapConfig> = use_signal(load_bootstrap_config);
    let sync_status = use_signal(|| SyncStatus::Offline);
    let sync_issue = use_signal(|| None::<String>);
    let last_sync_at = use_signal(|| None::<i64>);
    let pending_sync_count = use_signal(|| 0usize);
    let pending_sync_note_ids = use_signal(Vec::new);

    let mut sync_status_signal = sync_status;
    let mut sync_issue_signal = sync_issue;
    let last_sync_at_signal = last_sync_at;

    // Auth dependencies live in a signal so they can be reactively
    // populated once `bootstrap_config` resolves. Initial value carries
    // the always-present keyring store; the `auth_client` /
    // `api_base_url` fields fill in after bootstrap.
    let mut auth_deps: Signal<AuthDeps> = use_signal(|| AuthDeps {
        auth_client: None,
        token_store: Arc::new(KeyringTokenStore::new(KEYRING_SERVICE, KEYRING_ACCOUNT)),
        api_base_url: None,
    });

    use_effect(move || {
        let bootstrap = bootstrap_config();
        let api_base_url = bootstrap
            .dirt_api_base_url
            .as_deref()
            .filter(|value| !value.trim().is_empty());
        let (client_arc, normalized_url) =
            api_base_url.map_or((None, None), |url| match AuthClient::new(url) {
                Ok(client) => {
                    let normalized = client.base_url().to_string();
                    (Some(Arc::new(client)), Some(normalized))
                }
                Err(err) => {
                    tracing::error!("AuthClient build failed for `{url}`: {err}");
                    (None, None)
                }
            });
        auth_deps.with_mut(|deps| {
            deps.auth_client = client_arc;
            deps.api_base_url = normalized_url;
        });
    });

    // Initialize the local database; once it's ready, hydrate any
    // pre-existing session from the keyring and (only if a token is
    // present) spawn the auto-sync worker. A missing token leaves
    // sync_status at Offline — sync is opt-in.
    let _db_init_task = use_resource(move || async move {
        match DatabaseService::new().await {
            Ok(db) => {
                let db = Arc::new(db);

                let loaded_settings = match db.load_settings_with_large_stack().await {
                    Ok(settings) => settings,
                    Err(error) => {
                        tracing::error!("Failed to load desktop settings: {error}");
                        sync_status_signal.set(SyncStatus::Error);
                        sync_issue_signal
                            .set(Some(format!("Failed to load desktop settings: {error}")));
                        db_service.set(None);
                        sync_worker_started.set(false);
                        return;
                    }
                };
                let resolved_theme = resolve_theme(loaded_settings.theme);
                settings.set(loaded_settings);
                theme.set(resolved_theme);

                db_service.set(Some(db.clone()));

                if !sync_worker_started() {
                    sync_worker_started.set(true);
                    let deps = auth_deps.read().clone();
                    match hydrate_session(&deps) {
                        Ok(Some(session)) => {
                            let stored = deps.token_store.load().ok().flatten();
                            signed_in.set(stored);
                            let session_arc = Arc::new(session);
                            session_client.set(Some(session_arc.clone()));
                            spawn_session_worker(
                                db,
                                session_arc,
                                sync_worker,
                                sync_status_signal,
                                sync_issue_signal,
                                last_sync_at_signal,
                            );
                        }
                        Ok(None) => {
                            // No token in keyring — sync is opt-in. Leave
                            // the worker unspawned and the status at
                            // Offline; the user can sign in via Settings
                            // → Account when they want sync.
                            signed_in.set(None);
                            session_client.set(None);
                            sync_status_signal.set(SyncStatus::Offline);
                            sync_issue_signal.set(None);
                        }
                        Err(error) => {
                            tracing::error!("Session hydrate failed: {error}");
                            sync_status_signal.set(SyncStatus::Error);
                            sync_issue_signal.set(Some(error));
                        }
                    }
                }
            }
            Err(error) => {
                tracing::error!("Failed to initialize database: {error}");
                // Surface the failure on the same channel a sync error
                // would use. Without this the user sees a blank app and
                // no indication of what's wrong.
                sync_status_signal.set(SyncStatus::Error);
                sync_issue_signal.set(Some(format!("Database failed to open: {error}")));
                db_service.set(None);
                // Allow another bootstrap pass to retry — the
                // `use_resource` only re-runs if its read state changes,
                // so we have to release the gate ourselves.
                sync_worker_started.set(false);
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

    use_context_provider(|| auth_deps);
    use_context_provider(|| AppState {
        notes,
        current_note_id,
        search_query,
        active_tag_filter,
        settings,
        theme,
        db_service,
        transcription_service,
        signed_in,
        session_client,
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

/// Try to hydrate a `SessionApiClient` from the keyring.
///
/// Returns `Ok(None)` when sync is unconfigured (no auth client / no
/// base URL — sync just isn't available, not a loud failure) or when
/// the keyring slot is empty (user hasn't logged in yet). `Err(msg)`
/// is reserved for real misconfigurations the user should see in the
/// UI (malformed base URL, keyring backend failure).
fn hydrate_session(deps: &AuthDeps) -> Result<Option<SessionApiClient>, String> {
    let Some(auth_client) = deps.auth_client.as_ref() else {
        // No base URL configured — sync just isn't available. Not an
        // error: the app still works offline.
        return Ok(None);
    };
    let Some(base_url) = deps.api_base_url.as_ref() else {
        return Ok(None);
    };
    SessionApiClient::from_store(
        base_url.clone(),
        (**auth_client).clone(),
        deps.token_store.clone(),
    )
    .map_err(|err| err.to_string())
}

/// Spawn the sync worker and the UI-bridge consumer that drains its
/// status channel into Dioxus signals. Factored out so login/logout
/// flows can call into the same setup path without duplicating the
/// channel wiring.
// Reachable only from `app.rs` and `account_settings.rs`, but routed
// through the (private) `app` module — so `pub` would be misleadingly
// broad. The `redundant_pub_crate` lint would prefer plain `pub`
// because the enclosing module is private; we override it because the
// `pub(crate)` is documenting *intent* (callable anywhere inside
// dirt-desktop) rather than just satisfying the type checker.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn spawn_session_worker(
    db: Arc<DatabaseService>,
    session: Arc<SessionApiClient>,
    mut sync_worker_signal: Signal<Option<SyncWorkerHandle>>,
    mut sync_status_signal: Signal<SyncStatus>,
    mut sync_issue_signal: Signal<Option<String>>,
    mut last_sync_at_signal: Signal<Option<i64>>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SyncEvent>();
    let handle = spawn_sync_worker(db, session, tx);
    sync_worker_signal.set(Some(handle));
    spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                SyncEvent::Status(status) => sync_status_signal.set(status),
                SyncEvent::Issue(issue) => sync_issue_signal.set(issue),
                SyncEvent::LastSync(ts) => last_sync_at_signal.set(Some(ts)),
            }
        }
    });
    tracing::info!("Sync worker spawned");
}
