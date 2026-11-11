//! Mobile app shell.
//!
//! Owns startup wiring: open the local store, build an [`AuthClient`] +
//! [`DefaultTokenStore`] from the build-baked bootstrap (and provide
//! both via context as [`AuthDeps`]), and restore any saved session
//! from the `EncryptedSharedPreferences` slot.
//!
//! Signing in is optional: a missing or never-saved token is not an
//! error. The user can sign in via Settings → Account; the magic-link
//! request / verify flow lives in [`crate::views::settings`].
//!
//! Notes are not synced. They live in the local store on this device
//! whether or not the user is signed in.

use std::sync::Arc;

use dioxus::prelude::*;
use dirt_core::auth::{AuthClient, StoredToken};

use crate::bootstrap_config::{load_bootstrap_config, BootstrapConfig};
use crate::data::MobileNoteStore;
use crate::services::auth_flow::describe_store_error;
use crate::services::auth_store::DefaultTokenStore;
use crate::state::{AppState, AuthDeps, View};
use crate::ui::MOBILE_UI_STYLES;
use crate::views::{Editor, List, Settings};

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
    let startup_issue = use_signal(|| None::<String>);
    let store: Signal<Option<Arc<MobileNoteStore>>> = use_signal(|| None);
    let signed_in: Signal<Option<StoredToken>> = use_signal(|| None);

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
        startup_issue,
        store,
        signed_in,
    });
    use_context_provider(|| auth_deps_signal);

    // One-shot startup. `use_resource` only re-fires if a tracked
    // signal changes; `init_started` keeps the dance idempotent so
    // repeated re-renders don't open the store again.
    let mut init_started = use_signal(|| false);
    let mut store_w = store;
    let mut startup_issue_w = startup_issue;
    let mut notes_w = notes;
    let mut signed_in_w = signed_in;

    let _init = use_resource(move || async move {
        if init_started() {
            return;
        }
        init_started.set(true);

        let opened = match MobileNoteStore::open_default() {
            Ok(s) => Arc::new(s),
            Err(error) => {
                tracing::error!("Failed to open mobile DB: {error}");
                startup_issue_w.set(Some(format!("Failed to open local database: {error}")));
                init_started.set(false);
                return;
            }
        };

        match opened.list_notes().await {
            Ok(initial) => notes_w.set(initial),
            Err(error) => {
                tracing::warn!("Initial note listing failed: {error}");
            }
        }

        store_w.set(Some(opened));

        let deps = auth_deps_signal.read().clone();
        match load_saved_session(&deps) {
            // `None` means no token in the EncryptedSharedPreferences
            // slot: the user has not signed in, which is not an error.
            Ok(saved) => signed_in_w.set(saved),
            Err(error) => {
                tracing::error!("Saved session could not be read: {error}");
                startup_issue_w.set(Some(error));
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

/// Read the saved session token from the configured `TokenStore`.
///
/// Returns `Ok(None)` when sign-in is unconfigured (no auth client /
/// no base URL) or when the slot is empty (user hasn't logged in yet).
/// `Err(msg)` is reserved for a token store failure the user should
/// see in the UI.
fn load_saved_session(deps: &AuthDeps) -> Result<Option<StoredToken>, String> {
    if deps.auth_client.is_none() || deps.api_base_url.is_none() {
        return Ok(None);
    }
    deps.token_store
        .load()
        .map_err(|err| describe_store_error(&err))
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
