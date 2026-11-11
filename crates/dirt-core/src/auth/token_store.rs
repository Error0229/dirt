//! Pluggable persistent storage for the authenticated session.
//!
//! The trait is deliberately synchronous: the only two implementations
//! we ship are [`MemoryTokenStore`](super::MemoryTokenStore) (a process-
//! local `Mutex`) and [`KeyringTokenStore`](super::KeyringTokenStore)
//! (a thin wrapper over the `keyring` crate, whose own API is sync).
//! Keeping the trait sync also keeps it dyn-compatible without dragging
//! in `async-trait`. Callers in an async context that worry about the
//! keyring's `DBus` / Keychain IPC blocking the executor can wrap calls
//! in [`tokio::task::spawn_blocking`].
//!
//! A `TokenStore` is normally accessed once at startup (to hydrate the
//! current session) and again on login / refresh / logout. It is not on
//! the hot path of `push` / `pull`, so the sync cost is negligible.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Errors returned by a [`TokenStore`] implementation.
#[derive(Debug, Error)]
pub enum TokenStoreError {
    /// The underlying secret store (OS keyring, file, etc.) rejected the
    /// request. The message preserves the backend's own error string for
    /// diagnostics — these surface to users as "couldn't save your
    /// session", so the wrapping UI should treat them as opaque.
    #[error("token store backend error: {0}")]
    Backend(String),
    /// Serialization or deserialization of a `StoredToken` failed. In
    /// practice this means the stored blob was written by an older /
    /// newer build with an incompatible `StoredToken` shape; the caller
    /// should treat the slot as empty and force a fresh sign-in.
    #[error("token store serialization error: {0}")]
    Serialize(String),
}

pub type TokenStoreResult<T> = Result<T, TokenStoreError>;

/// A persisted authenticated session.
///
/// Field-for-field mirror of
/// [`VerifyResponse`](super::VerifyResponse) so the post-verify
/// happy-path can `From::from` directly into a `StoredToken`. On a
/// refresh, only `session_token`, `session_id`, and `expires_at_ms`
/// rotate — the caller is expected to copy `user_id` and `email`
/// forward from the previously-stored value via
/// [`StoredToken::with_refreshed`].
///
/// `ZeroizeOnDrop` is best-effort: it scrubs the heap allocation
/// owned by this `StoredToken` when the struct goes out of scope.
/// It does **not** cover transient `String`s produced by
/// `serde_json::to_string` / `from_str` while loading or saving, nor
/// any copies the caller has cloned. Treat it as defence in depth
/// against post-mortem memory snapshots, not a guarantee against a
/// live attacker with `/proc/<pid>/mem`.
///
/// `Debug` is intentionally **not** derived. An auto-derived `Debug`
/// would render `session_token` and `session_id` verbatim through any
/// `{:?}`-formatting site (`dbg!`, structured logging macros, panic
/// chains) — which is the exact log-aggregator leak pathway
/// `ZeroizeOnDrop` is meant to mitigate. The manual impl below
/// redacts both fields while keeping the non-secret identity fields
/// visible for diagnostics.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct StoredToken {
    pub session_token: String,
    pub session_id: String,
    pub user_id: String,
    pub email: String,
    pub expires_at_ms: i64,
}

impl std::fmt::Debug for StoredToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `session_token` is a live bearer credential; `session_id` is
        // a server-side row identifier that, while not itself an auth
        // token, is enough to scope a token-DB compromise back to a
        // specific user/session in log corpora. Both are redacted.
        f.debug_struct("StoredToken")
            .field("session_token", &"[redacted]")
            .field("session_id", &"[redacted]")
            .field("user_id", &self.user_id)
            .field("email", &self.email)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

impl StoredToken {
    /// Produce a new `StoredToken` with the three rotating fields from
    /// a [`RefreshResponse`](super::RefreshResponse) merged in over
    /// `self`. Keeps `user_id` / `email` from the original session.
    ///
    /// Takes the whole response by reference rather than three
    /// positional `impl Into<String>` parameters: `session_token` and
    /// `session_id` are both stringy and easy to silently swap at the
    /// call site under the old shape — typed input from `AuthClient`
    /// makes that mistake unrepresentable.
    #[must_use]
    pub fn with_refreshed(&self, resp: &super::RefreshResponse) -> Self {
        Self {
            session_token: resp.session_token.clone(),
            session_id: resp.session_id.clone(),
            user_id: self.user_id.clone(),
            email: self.email.clone(),
            expires_at_ms: resp.expires_at_ms,
        }
    }

    /// Returns `true` if the stored token's `expires_at_ms` is at or
    /// before the current wall-clock time.
    ///
    /// Lives here (instead of being duplicated across every consumer
    /// — `dirt-cli`, `dirt-desktop`, `dirt-mobile`) so the ms-vs-s
    /// unit convention is locked in by the struct itself. Use the
    /// return value to decide whether to call
    /// [`AuthClient::refresh_session`](super::AuthClient::refresh_session)
    /// before the next authed request.
    ///
    /// A clock that fails to read (mid-1970 epoch or a backwards
    /// system clock) is treated as "treat the token as expired and
    /// re-auth" — the alternative would be to silently keep using a
    /// possibly-expired session, which is worse than a forced refresh.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(i64::MAX);
        now_ms >= self.expires_at_ms
    }
}

impl From<super::VerifyResponse> for StoredToken {
    fn from(resp: super::VerifyResponse) -> Self {
        Self {
            session_token: resp.session_token,
            session_id: resp.session_id,
            user_id: resp.user_id,
            email: resp.email,
            expires_at_ms: resp.expires_at_ms,
        }
    }
}

/// Persistent storage for a [`StoredToken`].
///
/// Implementations must be `Send + Sync` so `Arc<dyn TokenStore>` is
/// the canonical "share a token store across tasks / windows" handle.
pub trait TokenStore: Send + Sync {
    /// Load the current stored token, or `None` if nothing is stored.
    /// A backend-level error (`DBus` offline, permission denied, etc.)
    /// returns `Err`; a missing entry returns `Ok(None)`.
    fn load(&self) -> TokenStoreResult<Option<StoredToken>>;

    /// Overwrite the stored token. Implementations should make this
    /// atomic from the caller's POV — a concurrent `load` should see
    /// either the old token or the new one, never a torn write.
    fn save(&self, token: &StoredToken) -> TokenStoreResult<()>;

    /// Remove the stored token. No-op if nothing is stored — must not
    /// error on `NoEntry`-style "already empty" cases.
    fn clear(&self) -> TokenStoreResult<()>;
}

/// Shared test fixture. Re-used by the memory and keyring store
/// test modules so they don't drift when `StoredToken` fields change.
#[cfg(test)]
pub(super) fn sample_stored_token() -> StoredToken {
    StoredToken {
        session_token: "tok".into(),
        session_id: "sid".into(),
        user_id: "uid".into(),
        email: "user@example.com".into(),
        expires_at_ms: 123,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{RefreshResponse, VerifyResponse};

    #[test]
    fn stored_token_from_verify_response_preserves_fields() {
        let resp = VerifyResponse {
            session_token: "tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 123,
        };
        let stored: StoredToken = resp.into();
        assert_eq!(stored.session_token, "tok");
        assert_eq!(stored.session_id, "sid");
        assert_eq!(stored.user_id, "uid");
        assert_eq!(stored.email, "user@example.com");
        assert_eq!(stored.expires_at_ms, 123);
    }

    #[test]
    fn with_refreshed_keeps_identity_and_rotates_session_fields() {
        let original = StoredToken {
            session_token: "old-tok".into(),
            session_id: "old-sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 100,
        };
        let resp = RefreshResponse {
            session_token: "new-tok".into(),
            session_id: "new-sid".into(),
            expires_at_ms: 200,
        };
        let refreshed = original.with_refreshed(&resp);
        assert_eq!(refreshed.session_token, "new-tok");
        assert_eq!(refreshed.session_id, "new-sid");
        assert_eq!(refreshed.expires_at_ms, 200);
        // identity is preserved
        assert_eq!(refreshed.user_id, original.user_id);
        assert_eq!(refreshed.email, original.email);
    }

    /// The manual `Debug` impl exists for one reason: keep the
    /// `session_token` out of log streams. Any future refactor that
    /// goes back to `#[derive(Debug)]` would silently leak the token
    /// to `tracing::debug!`, `dbg!`, and panic chains.
    #[test]
    fn debug_redacts_session_token_and_session_id() {
        let token = StoredToken {
            session_token: "supersecret-bearer-token".into(),
            session_id: "sess-deadbeef".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 123,
        };
        let rendered = format!("{token:?}");
        assert!(
            !rendered.contains("supersecret-bearer-token"),
            "session_token must not appear in Debug output: {rendered}"
        );
        assert!(
            !rendered.contains("sess-deadbeef"),
            "session_id must not appear in Debug output: {rendered}"
        );
        assert!(
            rendered.contains("[redacted]"),
            "redacted placeholder must be visible: {rendered}"
        );
        // Identity fields stay visible so log lines still help with
        // diagnosing "which user is this".
        assert!(rendered.contains("uid-1"));
        assert!(rendered.contains("user@example.com"));
    }

    #[test]
    fn stored_token_round_trips_through_json() {
        let original = StoredToken {
            session_token: "tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 123,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: StoredToken = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn is_expired_true_for_past_expiry() {
        let token = StoredToken {
            session_token: "tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            // Year 1971-ish — comfortably in the past for every test
            // run, both on a local machine and on CI runners.
            expires_at_ms: 1,
        };
        assert!(token.is_expired());
    }

    #[test]
    fn is_expired_false_for_future_expiry() {
        let token = StoredToken {
            session_token: "tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            // Year 4000-ish — comfortably in the future so a slow
            // CI runner that drifts ms between this and the call
            // won't flip the assertion.
            expires_at_ms: 64_060_588_800_000,
        };
        assert!(!token.is_expired());
    }
}
