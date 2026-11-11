//! Main application component.
//!
//! Notes are kept in the local store on this device; nothing syncs them.
//!
//! The session bearer lives in the OS-native keyring
//! ([`KeyringTokenStore`]), not in an environment variable. Signing in
//! through Settings → Account records who is signed in and has no other
//! effect.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::{window, LogicalPosition, LogicalSize};
use dioxus::prelude::*;
use dirt_core::auth::{AuthClient, KeyringTokenStore, StoredToken};
use dirt_core::models::Note;
use dirt_core::Store;

use crate::bootstrap_config::{load_bootstrap_config, BootstrapConfig};
use crate::components::{QuickCapture, SettingsPanel};
use crate::queries::use_notes_query;
use crate::services::open_local_store;
use crate::state::{AppState, AuthDeps};
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
    let mut db_service: Signal<Option<Arc<Store>>> = use_signal(|| None);
    let mut startup_error: Signal<Option<String>> = use_signal(|| None);
    let mut signed_in = use_signal(|| None);
    // Bootstrap is build-baked (see `bootstrap_config::load_bootstrap_config`)
    // and never changes at runtime. Loading it into a local rather than a
    // `Signal` lets us build `auth_deps` synchronously below: the
    // `_db_init_task` `use_resource` reads `auth_deps` early to restore
    // the signed-in account, and must see the final value when it does.
    let bootstrap = load_bootstrap_config();

    // Build the auth deps once from `bootstrap`. Same pattern as
    // `dirt-mobile::app_shell::build_auth_deps`: every field is
    // populated synchronously so any downstream `use_resource` /
    // `use_effect` reading the signal sees a final value on first
    // render. The signal is never mutated after init — it stays in a
    // `Signal` only so it can be shared via `use_context_provider`.
    let auth_deps: Signal<AuthDeps> = use_signal(|| build_auth_deps(&bootstrap));

    // Open the local store and load the settings; once that is done,
    // restore the signed-in account from the keyring.
    let _db_init_task = use_resource(move || async move {
        match open_local_store() {
            Ok(db) => {
                let db = Arc::new(db);

                let loaded_settings = match db.load_settings().await {
                    Ok(settings) => settings,
                    Err(error) => {
                        tracing::error!("Failed to load desktop settings: {error}");
                        startup_error.set(Some(format!("Could not read your settings: {error}")));
                        db_service.set(None);
                        return;
                    }
                };
                let resolved_theme = resolve_theme(loaded_settings.theme);
                settings.set(loaded_settings);
                theme.set(resolved_theme);

                db_service.set(Some(db));

                let stored = stored_session(&auth_deps.read());
                signed_in.set(stored);
            }
            Err(error) => {
                tracing::error!("Failed to initialize database: {error}");
                startup_error.set(Some(format!("Could not open your notes: {error}")));
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

    use_context_provider(|| auth_deps);
    use_context_provider(|| AppState {
        notes,
        current_note_id,
        search_query,
        active_tag_filter,
        settings,
        theme,
        db_service,
        startup_error,
        signed_in,
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

/// Build the per-process auth dependencies from the build-baked
/// bootstrap.
///
/// Synchronous on purpose: the `_db_init_task` `use_resource` reads
/// `auth_deps` as soon as the store is open, so every field must hold
/// its final value at signal init. `bootstrap` is constant for the
/// lifetime of the process, so there is nothing for the reactive layer
/// to react to anyway.
fn build_auth_deps(bootstrap: &BootstrapConfig) -> AuthDeps {
    let api_base_url = bootstrap
        .dirt_api_base_url
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let (auth_client, normalized_url) =
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
    AuthDeps {
        auth_client,
        token_store: Arc::new(KeyringTokenStore::new(KEYRING_SERVICE, KEYRING_ACCOUNT)),
        api_base_url: normalized_url,
    }
}

/// The session saved in the keyring, if there is one.
///
/// Returns `None` when no API base URL is configured: the Account tab
/// can then neither sign in nor sign out, so it does not show a saved
/// session as signed in.
fn stored_session(deps: &AuthDeps) -> Option<StoredToken> {
    deps.auth_client.as_ref()?;
    match deps.token_store.load() {
        Ok(stored) => stored,
        Err(error) => {
            tracing::error!("Failed to read the saved session: {error}");
            None
        }
    }
}
