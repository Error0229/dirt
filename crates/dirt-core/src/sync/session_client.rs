//! Sync-API client that silently rotates its bearer token on 401.
//!
//! Wraps a vanilla [`ApiClient`] with the machinery needed to recover
//! from an expired session without surfacing the failure to the UI:
//!
//!   1. Call the wrapped client (`current()` returns a cheap `Arc` clone).
//!   2. On `ApiClientError::Unauthorized`, the caller invokes
//!      [`SessionApiClient::refresh`].
//!   3. `refresh` hits `POST /v1/auth/refresh` with the current bearer,
//!      persists the new [`StoredToken`] via the injected
//!      [`TokenStore`], and atomically swaps the internal `ApiClient`
//!      so the next `current()` call returns one stamped with the
//!      fresh bearer.
//!
//! On a permanent refresh failure (`SESSION_EXPIRED` / no stored token)
//! the keyring slot is cleared so a stale credential can never be
//! reused, and the caller is expected to surface a re-auth prompt.
//!
//! Lives in `sync` rather than `auth` because the only consumer is the
//! sync engine — `dirt-cli` keeps its short-lived one-shot `AuthClient`
//! flows and does not need silent refresh.

use std::sync::{Arc, RwLock};

use crate::auth::{AuthClient, AuthError, TokenStore};
use crate::sync::api_client::ApiClient;

/// Errors surfaced from [`SessionApiClient::from_store`] /
/// [`SessionApiClient::refresh`].
///
/// Bucketed so the sync worker can branch on intent — `SessionExpired`
/// and `NoToken` are permanent (park sync, nudge the user to re-auth);
/// everything else is transient and safe to retry under the worker's
/// backoff schedule.
#[derive(Debug, thiserror::Error)]
pub enum SessionRefreshError {
    /// The store was empty at refresh time. Either the user signed out
    /// concurrently or never signed in — the worker should park until
    /// the next explicit kick.
    #[error("no stored token to refresh")]
    NoToken,
    /// The platform secret store rejected the load / save / clear call
    /// (`DBus` offline, permission denied, etc.). Transient at the OS
    /// level; the sync worker should retry under backoff.
    #[error("token store error: {0}")]
    Store(String),
    /// `POST /v1/auth/refresh` returned `SESSION_EXPIRED`. The bearer
    /// was revoked, replaced by a newer refresh, or past `expires_at`.
    /// The keyring has been cleared so a stale token cannot be reused;
    /// the caller must restart the magic-code flow.
    #[error("session expired: {0}")]
    SessionExpired(String),
    /// Any other refresh-side error (network, 5xx, contract drift).
    /// Transient at the API layer; safe to retry under backoff.
    #[error("refresh failed: {0}")]
    Other(String),
    /// Rebuilding the inner [`ApiClient`] from the fresh bearer failed.
    /// Almost always a configuration regression (base URL stopped
    /// parsing). Treat as transient at the network layer, but it
    /// usually needs an operator fix.
    #[error("rebuild client error: {0}")]
    Rebuild(String),
}

/// Refresh-capable wrapper around an [`ApiClient`].
///
/// Cloning is cheap on the outside (`Arc<SessionApiClient>` is the
/// canonical handle); internally the current `ApiClient` lives under a
/// `RwLock<Arc<_>>` so callers can take a snapshot for one sync cycle
/// without holding a lock across `.await` points.
pub struct SessionApiClient {
    inner: RwLock<Arc<ApiClient>>,
    auth: AuthClient,
    store: Arc<dyn TokenStore>,
    base_url: String,
}

impl std::fmt::Debug for SessionApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionApiClient")
            .field("base_url", &self.base_url)
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl SessionApiClient {
    /// Build a session client by hydrating the bearer from `store`.
    ///
    /// Returns `Ok(None)` when the store has no token — the caller is
    /// expected to park sync until a login lands one. Returns
    /// `Err(Rebuild)` if `base_url` can't be paired with the stored
    /// bearer to build an `ApiClient` (almost always a malformed base
    /// URL); the caller should surface the misconfiguration to the UI.
    pub fn from_store(
        base_url: impl Into<String>,
        auth: AuthClient,
        store: Arc<dyn TokenStore>,
    ) -> Result<Option<Self>, SessionRefreshError> {
        let base_url = base_url.into();
        let stored = store
            .load()
            .map_err(|err| SessionRefreshError::Store(err.to_string()))?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        let api = ApiClient::new(&base_url, &stored.session_token)
            .map_err(|err| SessionRefreshError::Rebuild(err.to_string()))?;
        Ok(Some(Self {
            inner: RwLock::new(Arc::new(api)),
            auth,
            store,
            base_url,
        }))
    }

    /// Expose the normalized base URL for diagnostics. The bearer
    /// never leaks through any accessor on this type.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Read the `user_id` of whichever session token is currently in
    /// the underlying store.
    ///
    /// Used by the sync workers' pre-sync mismatch guard: if the
    /// keyring slot has been rotated to a different account since
    /// this process opened its DB (e.g. another client logged in as
    /// a different user), the worker must refuse to push under the
    /// wrong scope. The store read is cheap on every platform we
    /// ship (in-process keyring cache after the first ACL grant).
    ///
    /// Returns `Ok(None)` if the slot has been cleared concurrently
    /// (logout from another client) — the caller should park sync
    /// rather than treat that as a mismatch.
    pub fn current_user_id(&self) -> Result<Option<String>, SessionRefreshError> {
        // Clone the user_id out before `tok` is dropped — `StoredToken`
        // is `ZeroizeOnDrop` so we can't move fields out of it.
        self.store
            .load()
            .map(|opt| opt.map(|tok| tok.user_id.clone()))
            .map_err(|err| SessionRefreshError::Store(err.to_string()))
    }

    /// Cheap snapshot of the live `ApiClient`. The returned `Arc` is
    /// safe to hold across `.await` points — a concurrent `refresh`
    /// only swaps the slot, never mutates the value behind the `Arc`
    /// the caller still owns.
    pub fn current(&self) -> Arc<ApiClient> {
        Arc::clone(
            &self
                .inner
                .read()
                .expect("SessionApiClient inner lock poisoned"),
        )
    }

    /// Refresh the bearer in place. On success, the internal
    /// `ApiClient` is replaced — the next `current()` call returns
    /// one stamped with the new token. On `SessionExpired`, the
    /// keyring slot is cleared before returning so a stale bearer
    /// cannot be reused.
    pub async fn refresh(&self) -> Result<(), SessionRefreshError> {
        let stored = self
            .store
            .load()
            .map_err(|err| SessionRefreshError::Store(err.to_string()))?
            .ok_or(SessionRefreshError::NoToken)?;

        let resp = match self.auth.refresh_session(&stored.session_token).await {
            Ok(resp) => resp,
            Err(AuthError::SessionExpired(msg)) => {
                // Compare-and-swap: only clear the slot if it still
                // holds the very token whose refresh just failed.
                //
                // Phase 2.8 made this slot shared across CLI + desktop
                // + mobile, so two clients can hit a 401 at roughly the
                // same time, both read `tok-A`, and race into refresh.
                // The first refresh succeeds and writes `tok-B`; the
                // second arrives at the server with `tok-A` (now
                // rotated) and gets `SESSION_EXPIRED`. Unconditionally
                // clearing in that arm would wipe the winner's
                // `tok-B`, putting every client back at "Sign in
                // again" despite a perfectly valid token sitting on
                // disk a moment ago.
                //
                // Surfacing `SessionExpired` to *this* caller is still
                // correct — its in-flight bearer is dead and the
                // worker should park. The slot's contents belong to
                // whoever wrote last.
                clear_if_still_stale(self.store.as_ref(), &stored.session_token);
                return Err(SessionRefreshError::SessionExpired(msg));
            }
            Err(other) => return Err(SessionRefreshError::Other(other.to_string())),
        };

        let refreshed = stored.with_refreshed(&resp);
        self.store
            .save(&refreshed)
            .map_err(|err| SessionRefreshError::Store(err.to_string()))?;
        let new_api = ApiClient::new(&self.base_url, &refreshed.session_token)
            .map_err(|err| SessionRefreshError::Rebuild(err.to_string()))?;
        *self
            .inner
            .write()
            .expect("SessionApiClient inner lock poisoned") = Arc::new(new_api);
        Ok(())
    }
}

/// Clear the keyring slot only if it still holds `stale_token`.
///
/// Pulled out of [`SessionApiClient::refresh`] so the compare-and-swap
/// stays scrutable: a peer process on the same keyring slot may have
/// rotated the token between our `load` and the server's
/// `SESSION_EXPIRED` reply, and clearing the peer's freshly-rotated
/// token would put every client back at "Sign in again."
///
/// Reading the store twice (once at the top of `refresh` and again
/// here) is a deliberate cost: it bounds the window in which a peer's
/// write can clobber ours to "between our load and our clear" rather
/// than the full duration of the network round-trip. The platform
/// keyring serializes individual reads/writes inside the OS, so the
/// load↔clear is the smallest atomic unit we can build without
/// reaching for an out-of-process lock.
fn clear_if_still_stale(store: &dyn TokenStore, stale_token: &str) {
    match store.load() {
        Ok(Some(current)) if current.session_token == stale_token => {
            if let Err(err) = store.clear() {
                tracing::warn!("Failed to clear stale token after SessionExpired: {err}");
            }
        }
        Ok(Some(_)) => {
            // Slot was rotated by a peer between our load and the
            // SESSION_EXPIRED reply — preserve the peer's token.
            tracing::info!(
                "Keyring slot rotated by a peer during refresh; preserving the peer's token after SessionExpired"
            );
        }
        Ok(None) => {
            // Slot already empty — nothing to do; another path cleared
            // it (logout, manual delete, etc.).
        }
        Err(err) => {
            tracing::warn!("Failed to inspect keyring during SessionExpired: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{MemoryTokenStore, StoredToken};
    use serde_json::json;
    use wiremock::matchers::{bearer_token, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const REFRESH_PATH: &str = "/v1/auth/refresh";

    fn seeded_store(token: &str) -> Arc<MemoryTokenStore> {
        Arc::new(MemoryTokenStore::with_initial(StoredToken {
            session_token: token.into(),
            session_id: "sid-old".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1,
        }))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn from_store_returns_none_when_store_empty() {
        let server = MockServer::start().await;
        let auth = AuthClient::new(server.uri()).unwrap();
        let store: Arc<dyn TokenStore> = Arc::new(MemoryTokenStore::new());
        let session = SessionApiClient::from_store(server.uri(), auth, store).unwrap();
        assert!(session.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_swaps_inner_client_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REFRESH_PATH))
            .and(bearer_token("old-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "new-token",
                "session_id": "sid-new",
                "expires_at_ms": 9_999,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let auth = AuthClient::new(server.uri()).unwrap();
        let store_concrete = seeded_store("old-token");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = SessionApiClient::from_store(server.uri(), auth, store)
            .unwrap()
            .expect("seeded store should hydrate a session");

        session.refresh().await.unwrap();
        let saved = store_concrete.load().unwrap().unwrap();
        assert_eq!(saved.session_token, "new-token");
        assert_eq!(saved.session_id, "sid-new");
        assert_eq!(saved.expires_at_ms, 9_999);
        // user_id / email are preserved from the original stored token.
        assert_eq!(saved.user_id, "uid-1");
        assert_eq!(saved.email, "user@example.com");
    }

    /// A 401 `SESSION_EXPIRED` from `/v1/auth/refresh` is the bearer's
    /// terminal state. The store must be cleared so a follow-up
    /// auto-refresh attempt can't keep retrying a known-dead credential.
    #[tokio::test(flavor = "current_thread")]
    async fn refresh_clears_store_and_surfaces_session_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REFRESH_PATH))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again to obtain a fresh session token.",
                }
            })))
            .mount(&server)
            .await;

        let auth = AuthClient::new(server.uri()).unwrap();
        let store_concrete = seeded_store("dead-token");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = SessionApiClient::from_store(server.uri(), auth, store)
            .unwrap()
            .unwrap();

        let err = session.refresh().await.unwrap_err();
        assert!(matches!(err, SessionRefreshError::SessionExpired(_)));
        assert!(
            store_concrete.load().unwrap().is_none(),
            "store must be cleared after SESSION_EXPIRED refresh"
        );
    }

    /// A transient refresh error (5xx, network blip) must NOT clear the
    /// store — the credential is still potentially valid and the worker
    /// will retry under backoff.
    #[tokio::test(flavor = "current_thread")]
    async fn refresh_preserves_store_on_transient_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REFRESH_PATH))
            .respond_with(ResponseTemplate::new(503).set_body_string("turso down"))
            .mount(&server)
            .await;

        let auth = AuthClient::new(server.uri()).unwrap();
        let store_concrete = seeded_store("still-valid");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = SessionApiClient::from_store(server.uri(), auth, store)
            .unwrap()
            .unwrap();

        let err = session.refresh().await.unwrap_err();
        assert!(matches!(err, SessionRefreshError::Other(_)));
        assert!(
            store_concrete.load().unwrap().is_some(),
            "store must NOT be cleared on transient refresh failure"
        );
    }

    /// Phase 2.8 made the keyring slot shared across CLI / desktop /
    /// mobile, which opens a refresh race: two clients hit a 401 on
    /// the *same* stale token, both enter `refresh`, the first
    /// succeeds and writes a fresh token, the second arrives at the
    /// server with the now-rotated token and gets `SESSION_EXPIRED`.
    /// The old behaviour cleared the slot unconditionally on
    /// `SESSION_EXPIRED`, wiping the winner's fresh token and putting
    /// every client back at "Sign in again."
    ///
    /// This regression test models the race precisely. `refresh()`
    /// reads the store twice: once at the top to pick the bearer to
    /// send, and once inside the CAS in `clear_if_still_stale` to
    /// decide whether the slot still holds the bearer that just
    /// failed. A peer's write lands between those two loads. The
    /// `LoadSwapStore` wrapper returns `tok-stale` on the first load
    /// (so refresh POSTs the stale bearer the server then rejects)
    /// and `tok-fresh` on every subsequent load (so the CAS sees the
    /// peer's freshly-rotated token and skips the clear).
    #[tokio::test(flavor = "current_thread")]
    async fn refresh_preserves_peer_rotated_token_during_concurrent_session_expired() {
        use std::sync::Mutex;

        use crate::auth::{StoredToken, TokenStore, TokenStoreError, TokenStoreResult};

        /// The first two `load()` calls (from `SessionApiClient::from_store`
        /// at construction, and the top of `refresh()` for the bearer
        /// it sends) return `tok-stale`. Every subsequent load returns
        /// `tok-fresh`, modeling a peer that won the refresh race
        /// between the top-of-refresh load and the CAS recheck.
        struct LoadSwapStore {
            load_count: Mutex<usize>,
            clear_count: Mutex<usize>,
            stale: StoredToken,
            fresh: StoredToken,
        }

        impl TokenStore for LoadSwapStore {
            fn load(&self) -> TokenStoreResult<Option<StoredToken>> {
                let mut n = self.load_count.lock().unwrap();
                *n += 1;
                if *n <= 2 {
                    Ok(Some(self.stale.clone()))
                } else {
                    Ok(Some(self.fresh.clone()))
                }
            }
            fn save(&self, _token: &StoredToken) -> TokenStoreResult<()> {
                Err(TokenStoreError::Backend(
                    "save() should never run on the SessionExpired path".into(),
                ))
            }
            fn clear(&self) -> TokenStoreResult<()> {
                *self.clear_count.lock().unwrap() += 1;
                Ok(())
            }
        }

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REFRESH_PATH))
            .and(bearer_token("tok-stale"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again to obtain a fresh session token.",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let auth = AuthClient::new(server.uri()).unwrap();
        let store_concrete = Arc::new(LoadSwapStore {
            load_count: Mutex::new(0),
            clear_count: Mutex::new(0),
            stale: StoredToken {
                session_token: "tok-stale".into(),
                session_id: "sid-old".into(),
                user_id: "uid-1".into(),
                email: "user@example.com".into(),
                expires_at_ms: 1,
            },
            fresh: StoredToken {
                session_token: "tok-fresh".into(),
                session_id: "sid-new".into(),
                user_id: "uid-1".into(),
                email: "user@example.com".into(),
                expires_at_ms: 9_999_999_999_999,
            },
        });
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = SessionApiClient::from_store(server.uri(), auth, store)
            .unwrap()
            .unwrap();

        let err = session.refresh().await.unwrap_err();
        assert!(matches!(err, SessionRefreshError::SessionExpired(_)));

        // The whole point of the CAS: the peer's `tok-fresh` was
        // already in the slot at clear-decision time, so `clear()`
        // must NOT have fired. Pre-fix this counter would be 1.
        assert_eq!(
            *store_concrete.clear_count.lock().unwrap(),
            0,
            "compare-and-swap must skip clear() when the slot holds a peer's freshly rotated token"
        );

        // Sanity: at least three loads must have happened — one each
        // from `from_store`, the top of `refresh()`, and the CAS
        // recheck inside `clear_if_still_stale`. Anything less proves
        // the CAS branch never ran and the clear-skipped assertion
        // above is hollow.
        assert!(
            *store_concrete.load_count.lock().unwrap() >= 3,
            "expected at least three store.load() calls (from_store + refresh + CAS recheck)"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_returns_no_token_when_store_empty() {
        let server = MockServer::start().await;
        let auth = AuthClient::new(server.uri()).unwrap();
        // Build the session with a seeded store, then clear it out so
        // `refresh` hits an empty slot — exercises the NoToken arm.
        let store_concrete = seeded_store("temporary");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = SessionApiClient::from_store(server.uri(), auth, store)
            .unwrap()
            .unwrap();
        store_concrete.clear().unwrap();

        let err = session.refresh().await.unwrap_err();
        assert!(matches!(err, SessionRefreshError::NoToken));
    }
}
