//! Pre-sync scope mismatch guard.
//!
//! The keyring slot `(dev.dirt.session, default)` is shared across the
//! CLI, desktop, and mobile clients by design — a login from one is
//! visible to all. After Phase 2.x per-user DB partitioning, the local
//! DB is bound to a specific user (`DatabaseService::user_id`), but the
//! keyring slot can be rotated by a peer process at any time. Without
//! a guard, a sync cycle in this process can pick up the peer's
//! bearer and push the local user's rows under the wrong identity —
//! exactly the cross-account leak the partitioning is meant to close.
//!
//! Every sync cycle (worker tick or one-shot CLI `dirt sync`) calls
//! [`check_scope`] before constructing a [`SyncEngine`]. On mismatch
//! the worker / CLI surfaces a [`ScopeMismatch`] error pointing the
//! user at "restart / re-login" rather than silently pushing.
//!
//! Returning `Ok(None)` from the underlying store load is also a
//! mismatch from sync's POV — there is no bearer to push with — but
//! the worker already treats that as "session vanished, park"; this
//! module only fires on a *populated but wrong* slot. The two cases
//! produce different errors so callers can surface the right copy.

use crate::sync::session_client::{SessionApiClient, SessionRefreshError};

/// Per-sync-cycle scope check result.
#[derive(Debug, thiserror::Error)]
pub enum ScopeCheckError {
    /// The keyring slot holds a bearer for a different user than the
    /// one this process's DB was opened for. The sync engine must
    /// not push under the peer's identity — surface to the user.
    #[error(
        "local DB belongs to {db_user} but the active session is for {session_user}; \
         restart this client (or sign in again) to switch accounts"
    )]
    Mismatch {
        db_user: String,
        session_user: String,
    },
    /// The token store had been cleared between login and this
    /// check (logout from another client). Sync should park; this is
    /// not a security signal, just "no bearer available."
    #[error("no session token in the keyring; sign in to resume sync")]
    SessionVanished,
    /// The token store backend itself failed (`DBus` down, etc.).
    #[error("failed to read session for scope check: {0}")]
    Store(String),
}

/// Compare `db.user_id()` against the bearer's `stored.user_id`.
///
/// `Ok(())` means it's safe to construct a `SyncEngine` and push.
/// Anything else means the worker must refuse this cycle and surface
/// the error.
pub fn check_scope(db_user_id: &str, session: &SessionApiClient) -> Result<(), ScopeCheckError> {
    match session.current_user_id() {
        Ok(Some(session_user)) if session_user == db_user_id => Ok(()),
        Ok(Some(session_user)) => Err(ScopeCheckError::Mismatch {
            db_user: db_user_id.to_string(),
            session_user,
        }),
        Ok(None) => Err(ScopeCheckError::SessionVanished),
        Err(SessionRefreshError::Store(msg)) => Err(ScopeCheckError::Store(msg)),
        Err(other) => Err(ScopeCheckError::Store(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::auth::{AuthClient, MemoryTokenStore, StoredToken, TokenStore};
    use wiremock::MockServer;

    const DB_USER: &str = "01932aaa-0000-7000-8000-000000000001";
    const PEER_USER: &str = "01932bbb-0000-7000-8000-000000000002";

    async fn session_with_user(user_id: &str) -> (SessionApiClient, Arc<MemoryTokenStore>) {
        let server = MockServer::start().await;
        let auth = AuthClient::new(server.uri()).expect("auth client builds");
        let store_concrete = Arc::new(MemoryTokenStore::with_initial(StoredToken {
            session_token: "tok".into(),
            session_id: "sid".into(),
            user_id: user_id.into(),
            email: "u@example.com".into(),
            expires_at_ms: 1_700_000_000_000,
        }));
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = SessionApiClient::from_store(server.uri(), auth, store)
            .expect("from_store ok")
            .expect("seeded store hydrates");
        (session, store_concrete)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn check_scope_ok_when_db_user_matches_session_user() {
        let (session, _store) = session_with_user(DB_USER).await;
        check_scope(DB_USER, &session).expect("matched users must pass");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn check_scope_mismatch_when_users_differ() {
        let (session, _store) = session_with_user(PEER_USER).await;
        let err = check_scope(DB_USER, &session).unwrap_err();
        match err {
            ScopeCheckError::Mismatch {
                db_user,
                session_user,
            } => {
                assert_eq!(db_user, DB_USER);
                assert_eq!(session_user, PEER_USER);
            }
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn check_scope_reports_session_vanished_when_store_empty() {
        let (session, store) = session_with_user(DB_USER).await;
        store.clear().unwrap();
        let err = check_scope(DB_USER, &session).unwrap_err();
        assert!(matches!(err, ScopeCheckError::SessionVanished));
    }
}
