//! Mobile app shell.
//!
//! Owns startup wiring: open the local DB, build the `ApiClient` from
//! `DIRT_API_BASE_URL` (env override or build-baked) + `DIRT_CLIENT_TOKEN`
//! (env-only — never baked), spawn the sync worker, and drain the
//! worker's `SyncEvent` mpsc into Dioxus signals on the UI side.
//!
//! Sync is fully background: there is no manual "Sync now" button.
//! Mutation sites (the editor's save/delete handlers) call
//! `AppState::trigger_sync()` after a successful local write, the
//! worker debounces a burst of edits into one round-trip, and the
//! status banner reflects whatever the worker most recently emitted.
//!
//! Misconfiguration is loud: a missing or empty `DIRT_CLIENT_TOKEN`
//! parks `SyncStatus::Error` and a human-readable message in
//! `sync_issue`. The list view surfaces both so the user can fix env
//! setup instead of running silently offline.

use std::sync::Arc;

use dioxus::prelude::*;
use dirt_core::sync::api_client::ApiClient;

use crate::bootstrap_config::{load_bootstrap_config, BootstrapConfig};
use crate::data::MobileNoteStore;
use crate::services::{spawn_sync_worker, SyncEvent, SyncWorkerHandle};
use crate::state::{AppState, SyncStatus, View};
use crate::ui::MOBILE_UI_STYLES;
use crate::views::{Editor, List};

#[component]
pub fn AppShell() -> Element {
    let notes = use_signal(Vec::new);
    let selected_note_id = use_signal(|| None);
    let view = use_signal(|| View::List);
    let sync_status = use_signal(|| SyncStatus::Offline);
    let sync_issue = use_signal(|| None::<String>);
    let last_sync_at = use_signal(|| None::<i64>);
    let store: Signal<Option<Arc<MobileNoteStore>>> = use_signal(|| None);
    let sync_worker: Signal<Option<SyncWorkerHandle>> = use_signal(|| None);

    use_context_provider(|| AppState {
        notes,
        selected_note_id,
        view,
        sync_status,
        sync_issue,
        last_sync_at,
        store,
        sync_worker,
    });

    // One-shot startup. `use_resource` only re-fires if a tracked
    // signal changes; `init_started` keeps the dance idempotent so
    // repeated re-renders don't try to spawn the worker again.
    let mut init_started = use_signal(|| false);
    let mut store_w = store;
    let mut sync_worker_w = sync_worker;
    let mut sync_status_w = sync_status;
    let mut sync_issue_w = sync_issue;
    let mut last_sync_at_w = last_sync_at;
    let mut notes_w = notes;

    let _init = use_resource(move || async move {
        if init_started() {
            return;
        }
        init_started.set(true);

        let opened = match MobileNoteStore::open_default().await {
            Ok(s) => Arc::new(s),
            Err(error) => {
                tracing::error!("Failed to open mobile DB: {error}");
                sync_status_w.set(SyncStatus::Error);
                sync_issue_w.set(Some(format!("Failed to open local database: {error}")));
                init_started.set(false);
                return;
            }
        };

        // Seed the note list so the UI has something to render before
        // the first sync cycle returns. List view will refresh again
        // after every successful sync via the SyncEvent stream.
        match opened.list_notes().await {
            Ok(initial) => notes_w.set(initial),
            Err(error) => {
                tracing::warn!("Initial note listing failed: {error}");
            }
        }

        store_w.set(Some(opened.clone()));

        let bootstrap = load_bootstrap_config();
        match build_api_client(&bootstrap) {
            Ok(api) => {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SyncEvent>();
                let handle = spawn_sync_worker(opened.clone(), Arc::new(api), tx);
                sync_worker_w.set(Some(handle));

                // Drain status events into signals. Using a separate
                // task here (rather than polling) keeps the UI thread
                // free of mpsc work and means the status updates land
                // as soon as the worker emits them.
                let store_for_refresh = opened.clone();
                spawn(async move {
                    while let Some(event) = rx.recv().await {
                        match event {
                            SyncEvent::Status(status) => sync_status_w.set(status),
                            SyncEvent::Issue(issue) => sync_issue_w.set(issue),
                            SyncEvent::LastSync(ts) => {
                                last_sync_at_w.set(Some(ts));
                                // A successful sync may have applied
                                // pulled rows; refresh the list so
                                // remote changes show up without a
                                // manual reload.
                                if let Ok(refreshed) = store_for_refresh.list_notes().await {
                                    notes_w.set(refreshed);
                                }
                            }
                        }
                    }
                });

                tracing::info!("Mobile sync worker spawned");
            }
            Err(error) => {
                tracing::error!("Sync worker not started: {error}");
                sync_status_w.set(SyncStatus::Error);
                sync_issue_w.set(Some(error));
            }
        }
    });

    let current_view = view();

    rsx! {
        style { "{MOBILE_UI_STYLES}" }
        div {
            style: "
                min-height: 100vh;
                font-family: system-ui, -apple-system, sans-serif;
                color: #111827;
                background: #f9fafb;
                display: flex;
                flex-direction: column;
            ",
            match current_view {
                View::List => rsx! { List {} },
                View::Editor => rsx! { Editor {} },
            }
        }
    }
}

/// Resolve API base URL + client token at app launch.
///
/// `DIRT_API_BASE_URL` falls back to the build-baked bootstrap value so
/// a packaged APK works without runtime env config. `DIRT_CLIENT_TOKEN`
/// is runtime-only — baking a bearer token into a binary that ships to
/// users is a non-starter. A missing or empty token is a hard error:
/// the worker won't start, the banner shows `Error`, and the user has
/// to fix their environment rather than the app silently running
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
