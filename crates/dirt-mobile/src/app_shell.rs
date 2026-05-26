//! Mobile app shell.
//!
//! Owns startup wiring: open the local DB, build an [`AuthClient`] +
//! [`DefaultTokenStore`] from the build-baked bootstrap (and provide
//! both via context as [`AuthDeps`]), hydrate any pre-existing session
//! from the `EncryptedSharedPreferences` slot, spawn the auto-sync
//! worker if a token is present, and drain the worker's `SyncEvent`
//! mpsc into Dioxus signals on the UI side.
//!
//! Auth is opt-in in the same sense as desktop: a missing or
//! never-saved token leaves `SyncStatus::Offline` and does NOT park
//! `SyncStatus::Error`. The user can sign in via Settings → Account
//! when they want sync; the magic-link request / verify flow lives in
//! [`crate::views::settings`].
//!
//! Sync is fully background: there is no manual "Sync now" button.
//! Mutation sites (the editor's save / delete handlers) call
//! [`crate::state::AppState::trigger_sync`] after a successful local
//! write; the worker debounces a burst of edits into one round-trip,
//! and the status banner reflects whatever the worker most recently
//! emitted.

use std::sync::Arc;

use dioxus::prelude::*;
use dirt_core::auth::{AuthClient, StoredToken};
use dirt_core::services::db_paths::{migrate_solo_db_to_user, read_active_user, write_active_user};
use dirt_core::sync::session_client::SessionApiClient;

use crate::bootstrap_config::{load_bootstrap_config, BootstrapConfig};
use crate::config::default_mobile_data_directory;
use crate::data::MobileNoteStore;
use crate::services::auth_store::DefaultTokenStore;
use crate::services::{spawn_sync_worker, SyncEvent, SyncWorkerHandle};
use crate::state::{AppState, AuthDeps, SyncStatus, View};
use crate::ui::MOBILE_UI_STYLES;
use crate::views::{Editor, List, Settings};
use tokio::sync::mpsc::UnboundedSender;

/// Keyring service identifier for the mobile session token. Matches
/// the values used in `dirt-cli` and `dirt-desktop` so the trio share
/// one logical slot — see `reference_keyring_slot.md` in the auto-
/// memory store and the module-level comment on
/// [`dirt_core::auth::keyring_store`].
const SESSION_PREFS_NAME: &str = "dev.dirt.session";
/// Per-user discriminator inside the preferences file. Solo phase
/// uses a static `"default"`; multi-user / multi-account would pivot
/// to a user identifier.
const SESSION_PREFS_KEY: &str = "default";

#[component]
pub fn AppShell() -> Element {
    let bootstrap = load_bootstrap_config();

    let notes = use_signal(Vec::new);
    let selected_note_id = use_signal(|| None);
    let view = use_signal(|| View::List);
    let sync_status = use_signal(|| SyncStatus::Offline);
    let sync_issue = use_signal(|| None::<String>);
    let last_sync_at = use_signal(|| None::<i64>);
    let store: Signal<Option<Arc<MobileNoteStore>>> = use_signal(|| None);
    let sync_worker: Signal<Option<SyncWorkerHandle>> = use_signal(|| None);
    let signed_in: Signal<Option<StoredToken>> = use_signal(|| None);
    let session_client: Signal<Option<Arc<SessionApiClient>>> = use_signal(|| None);
    let events_tx: Signal<Option<UnboundedSender<SyncEvent>>> = use_signal(|| None);

    // Auth deps are built once at startup — mobile bootstrap is
    // build-time-baked, so unlike desktop there's no async manifest
    // resolver to wait on. Failures to construct the `TokenStore` here
    // park the deps with `token_store = None`-equivalent behaviour:
    // the actual error is logged and surfaces through the Account row.
    let auth_deps_signal: Signal<AuthDeps> = use_signal(|| build_auth_deps(&bootstrap));

    use_context_provider(|| AppState {
        notes,
        selected_note_id,
        view,
        sync_status,
        sync_issue,
        last_sync_at,
        store,
        sync_worker,
        signed_in,
        session_client,
        events_tx,
    });
    use_context_provider(|| auth_deps_signal);

    // One-shot startup. `use_resource` only re-fires if a tracked
    // signal changes; `init_started` keeps the dance idempotent so
    // repeated re-renders don't try to spawn the worker again.
    let mut init_started = use_signal(|| false);
    let mut store_w = store;
    let mut sync_status_w = sync_status;
    let mut sync_issue_w = sync_issue;
    let mut last_sync_at_w = last_sync_at;
    let mut notes_w = notes;
    let mut signed_in_w = signed_in;
    let mut session_client_w = session_client;
    let mut events_tx_w = events_tx;

    let _init = use_resource(move || async move {
        if init_started() {
            return;
        }
        init_started.set(true);

        // Resolve which DB this launch should open (issue #234).
        // Mirrors desktop's branching:
        //   - Keyring populated AND state.json points to that user →
        //     open the per-user DB.
        //   - Keyring populated AND no state.json (or different user)
        //     → first-signin migration + pointer write, then open the
        //     per-user DB.
        //   - Keyring empty AND state.json present → honor the pointer
        //     (sign-out preserves DB allegiance).
        //   - Keyring empty AND no state.json → legacy solo DB.
        //
        // `app_shell` is `#[cfg(target_os = "android")]` per
        // `main.rs`, so the migration / store APIs we call here are
        // always available on this build.
        let opened = {
            let deps_for_init = auth_deps_signal.read().clone();
            let stored_token = deps_for_init.token_store.load().ok().flatten();
            let data_dir = default_mobile_data_directory();
            let active_pointer = read_active_user(&data_dir).await.ok().flatten();

            let result = if let Some(token) = &stored_token {
                let needs_update = active_pointer.as_deref() != Some(token.user_id.as_str());
                if needs_update {
                    if let Err(err) = migrate_solo_db_to_user(&data_dir, &token.user_id).await {
                        tracing::error!("Mobile solo→user migration failed: {err}");
                    }
                    if let Err(err) = write_active_user(&data_dir, &token.user_id).await {
                        tracing::error!("Could not write active-user pointer: {err}");
                    }
                }
                MobileNoteStore::open_for_user(&token.user_id).await
            } else {
                match active_pointer {
                    Some(uid) => MobileNoteStore::open_for_user(&uid).await,
                    None => MobileNoteStore::open_solo().await,
                }
            };

            match result {
                Ok(s) => Arc::new(s),
                Err(error) => {
                    tracing::error!("Failed to open mobile DB: {error}");
                    sync_status_w.set(SyncStatus::Error);
                    sync_issue_w.set(Some(format!("Failed to open local database: {error}")));
                    init_started.set(false);
                    return;
                }
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

        // Build the long-lived `SyncEvent` bridge here so the drainer
        // attaches to `AppShell`'s scope (the root). A previous version
        // created the channel + drainer inside `spawn_session_worker`,
        // which meant the drainer inherited the scope of *whatever
        // component called the helper* — for re-spawns from Settings
        // after sign-in that scope was the Settings view, so navigating
        // back to the list cancelled the drainer and silently dropped
        // every subsequent worker event.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SyncEvent>();
        events_tx_w.set(Some(tx.clone()));

        // Drainer task. Captures the signals + a store ref by clone so
        // the worker can also list freshly-pulled notes back into the
        // UI on every successful sync.
        let store_for_refresh = opened.clone();
        spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    SyncEvent::Status(status) => sync_status_w.set(status),
                    SyncEvent::Issue(issue) => sync_issue_w.set(issue),
                    SyncEvent::LastSync(ts) => {
                        last_sync_at_w.set(Some(ts));
                        if let Ok(refreshed) = store_for_refresh.list_notes().await {
                            notes_w.set(refreshed);
                        }
                    }
                }
            }
        });

        let deps = auth_deps_signal.read().clone();
        match hydrate_session(&deps) {
            Ok(Some(session)) => {
                let stored = deps.token_store.load().ok().flatten();
                signed_in_w.set(stored);
                let session_arc = Arc::new(session);
                session_client_w.set(Some(session_arc.clone()));
                spawn_session_worker(opened.clone(), session_arc, tx, sync_worker);
                tracing::info!("Mobile sync worker spawned");
            }
            Ok(None) => {
                // No token in the EncryptedSharedPreferences slot — sync
                // is opt-in. Leave the worker unspawned and the status
                // at Offline; the user can sign in via Settings →
                // Account when they want sync. `tx` is preserved on the
                // signal so the post-login spawn can pick it up.
                signed_in_w.set(None);
                session_client_w.set(None);
                sync_status_w.set(SyncStatus::Offline);
                sync_issue_w.set(None);
            }
            Err(error) => {
                tracing::error!("Session hydrate failed: {error}");
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
                View::Settings => rsx! { Settings {} },
            }
        }
    }
}

/// Build the [`AuthDeps`] context value from the bootstrap manifest.
///
/// `token_store` is always populated — `DefaultTokenStore::open` is
/// fallible on Android (the master-key materialization talks to the
/// `KeyStore`) so a failure here is logged + surfaced through the
/// Account row by leaving `auth_client` / `api_base_url` populated but
/// the token store unconstructable. We don't have a "no token store"
/// state in `AuthDeps` because every binary needs to be able to clear
/// the slot on logout — instead, a construction failure parks the
/// store as a sentinel that reports `Backend` on every call.
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

    let token_store: Arc<dyn dirt_core::auth::TokenStore> =
        match DefaultTokenStore::open(SESSION_PREFS_NAME, SESSION_PREFS_KEY) {
            Ok(store) => Arc::new(store),
            Err(err) => {
                // Falling all the way back to no-store would silently
                // break the Account row (no way to save / clear), so
                // surface it loudly. The Settings view checks for
                // `auth_client.is_none()` AND walks the store on each
                // operation, so the store-backed error will reach the
                // user as a Backend message on first interaction.
                tracing::error!("DefaultTokenStore::open failed: {err}");
                Arc::new(SentinelTokenStore::new(err.to_string()))
            }
        };

    AuthDeps {
        auth_client,
        token_store,
        api_base_url: normalized_url,
    }
}

/// Try to hydrate a `SessionApiClient` from the configured `TokenStore`.
///
/// Returns `Ok(None)` when sync is unconfigured (no auth client / no
/// base URL — sync just isn't available, not a loud failure) or when
/// the keyring slot is empty (user hasn't logged in yet). `Err(msg)`
/// is reserved for real misconfigurations the user should see in the
/// UI (malformed base URL, keyring backend failure).
fn hydrate_session(deps: &AuthDeps) -> Result<Option<SessionApiClient>, String> {
    let Some(auth_client) = deps.auth_client.as_ref() else {
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

/// Spawn the sync worker against an existing `SyncEvent` channel.
///
/// The channel + drainer are owned by [`AppShell`] (so the drainer
/// task lives in the root scope); this helper just plugs a new worker
/// into the existing pipe. Each call takes a fresh clone of the
/// long-lived sender — when the worker shuts down (sign-out) its
/// sender drops, but `AppShell` keeps its own clone so the drainer
/// stays alive for the next sign-in.
///
/// Factored out (rather than inlined) so login / logout flows in
/// [`crate::views::settings`] can call it without duplicating the
/// sender-clone dance.
// Reachable only from `app_shell.rs` and `views/settings.rs`, but
// routed through the (private) module — so `pub` would be misleadingly
// broad. The `redundant_pub_crate` lint would prefer plain `pub`
// because the enclosing module is private; we override it because the
// `pub(crate)` is documenting *intent* (callable anywhere inside
// dirt-mobile) rather than just satisfying the type checker.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn spawn_session_worker(
    store: Arc<MobileNoteStore>,
    session: Arc<SessionApiClient>,
    events_tx: UnboundedSender<SyncEvent>,
    mut sync_worker_signal: Signal<Option<SyncWorkerHandle>>,
) {
    let handle = spawn_sync_worker(store, session, events_tx);
    sync_worker_signal.set(Some(handle));
}

/// Sentinel `TokenStore` produced when the real store fails to open at
/// startup (Android `KeyStore` unreachable, `AndroidX` class not found,
/// etc.). Every call returns the cached failure message so the
/// Account row surfaces the same diagnostic on every interaction
/// rather than silently behaving as "no slot exists".
struct SentinelTokenStore {
    reason: String,
}

impl SentinelTokenStore {
    const fn new(reason: String) -> Self {
        Self { reason }
    }
}

impl dirt_core::auth::TokenStore for SentinelTokenStore {
    fn load(&self) -> dirt_core::auth::TokenStoreResult<Option<dirt_core::auth::StoredToken>> {
        Err(dirt_core::auth::TokenStoreError::Backend(
            self.reason.clone(),
        ))
    }

    fn save(&self, _token: &dirt_core::auth::StoredToken) -> dirt_core::auth::TokenStoreResult<()> {
        Err(dirt_core::auth::TokenStoreError::Backend(
            self.reason.clone(),
        ))
    }

    fn clear(&self) -> dirt_core::auth::TokenStoreResult<()> {
        Err(dirt_core::auth::TokenStoreError::Backend(
            self.reason.clone(),
        ))
    }
}
