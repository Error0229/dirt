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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredToken {
    pub session_token: String,
    pub session_id: String,
    pub user_id: String,
    pub email: String,
    pub expires_at_ms: i64,
}

impl StoredToken {
    /// Produce a new `StoredToken` with the three rotating fields from
    /// a [`RefreshResponse`](super::RefreshResponse) merged in over
    /// `self`. Keeps `user_id` / `email` from the original session.
    #[must_use]
    pub fn with_refreshed(
        &self,
        session_token: impl Into<String>,
        session_id: impl Into<String>,
        expires_at_ms: i64,
    ) -> Self {
        Self {
            session_token: session_token.into(),
            session_id: session_id.into(),
            user_id: self.user_id.clone(),
            email: self.email.clone(),
            expires_at_ms,
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::VerifyResponse;

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
        let refreshed = original.with_refreshed("new-tok", "new-sid", 200);
        assert_eq!(refreshed.session_token, "new-tok");
        assert_eq!(refreshed.session_id, "new-sid");
        assert_eq!(refreshed.expires_at_ms, 200);
        // identity is preserved
        assert_eq!(refreshed.user_id, original.user_id);
        assert_eq!(refreshed.email, original.email);
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
}
