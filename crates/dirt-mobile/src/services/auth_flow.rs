//! Pure auth-flow helpers (validate / describe / perform), extracted
//! from `views::settings` so they run on the host under `cargo test`.
//!
//! `views::settings` lives behind `#[cfg(target_os = "android")]`
//! because the Dioxus `#[component]` macros only compile against the
//! mobile dioxus dep. The helpers in this module have no Dioxus
//! dependency — they take `AuthDeps` and return data — so they can
//! exercise the request / verify / logout edges against a wiremock
//! server without an Android emulator. Match the desktop shape one-for-
//! one so a future shared library can drop them into both.

use std::sync::Arc;

use dirt_core::auth::{AuthError, StoredToken, TokenStoreError};
use dirt_core::sync::session_client::SessionApiClient;

use crate::state::AuthDeps;

/// Outcome of `verify_magic_code` + downstream persistence + session
/// client construction. The UI layer matches on this to either drive
/// the post-login worker startup or surface a Failure message.
pub enum LoginOutcome {
    Success(StoredToken, Arc<SessionApiClient>),
    Failure(String),
}

pub enum LogoutOutcome {
    Success,
    Failure(String),
}

/// `POST /v1/auth/request` against the magic-link endpoint and return
/// the resulting `request_id` (which the UI feeds into the verify
/// step). Wraps `AuthError` into a user-facing string so the UI doesn't
/// need to match on the full taxonomy.
pub async fn send_magic_code(deps: &AuthDeps, email: &str) -> Result<String, String> {
    let auth = deps
        .auth_client
        .as_ref()
        .ok_or_else(|| "Sign-in is unavailable: DIRT_API_BASE_URL not configured.".to_string())?;
    match auth.request_magic_code(email).await {
        Ok(resp) => Ok(resp.request_id),
        Err(err) => Err(describe_auth_error(&err)),
    }
}

/// `POST /v1/auth/verify` with the user-entered 6-digit code, persist
/// the resulting `StoredToken`, and build a `SessionApiClient` ready
/// for the sync worker.
pub async fn perform_verify(deps: &AuthDeps, request_id: &str, code: &str) -> LoginOutcome {
    let Some(auth) = deps.auth_client.as_ref() else {
        return LoginOutcome::Failure(
            "Sign-in is unavailable: DIRT_API_BASE_URL not configured.".into(),
        );
    };
    let resp = match auth.verify_magic_code(request_id, code).await {
        Ok(resp) => resp,
        Err(err) => return LoginOutcome::Failure(describe_auth_error(&err)),
    };
    let stored: StoredToken = resp.into();
    if let Err(err) = deps.token_store.save(&stored) {
        return LoginOutcome::Failure(describe_store_error(&err));
    }
    let Some(base_url) = deps.api_base_url.clone() else {
        return LoginOutcome::Failure(
            "Sign-in succeeded but the API base URL is missing — sync cannot start.".into(),
        );
    };
    match SessionApiClient::from_store(base_url, (**auth).clone(), deps.token_store.clone()) {
        Ok(Some(session)) => LoginOutcome::Success(stored, Arc::new(session)),
        Ok(None) => LoginOutcome::Failure(
            "Token was saved but the keystore read back empty. Try signing in again.".into(),
        ),
        Err(err) => LoginOutcome::Failure(format!("Could not build sync client: {err}")),
    }
}

/// Revoke the server-side session, then clear the local slot.
///
/// Reads the *freshest* bearer from the store rather than reusing a
/// snapshot — silent refresh in the sync worker rotates the token
/// without touching the UI's `signed_in` signal, so a stale snapshot
/// would `POST /v1/auth/logout` with an already-revoked token, receive
/// `SESSION_EXPIRED`, treat that as success, and leave the live token
/// on the server until it expired naturally.
///
/// Refuses to clear the local slot when `auth_client` is `None` — the
/// only way to reach that branch is a CLI / desktop login → mobile
/// open sequence where `DIRT_API_BASE_URL` is missing on the mobile
/// side. Silently wiping local would orphan the server session past
/// the point where the user has any handle to revoke it.
pub async fn perform_logout(deps: &AuthDeps) -> LogoutOutcome {
    let current = match deps.token_store.load() {
        Ok(Some(token)) => token,
        Ok(None) => return LogoutOutcome::Success,
        Err(err) => return LogoutOutcome::Failure(describe_store_error(&err)),
    };

    let Some(auth) = deps.auth_client.as_ref() else {
        return LogoutOutcome::Failure(
            "Cannot sign out: DIRT_API_BASE_URL is not configured on this app, \
             so the server session can't be revoked. Rebuild the APK with the \
             base URL set, or run `dirt auth logout` from the CLI."
                .into(),
        );
    };

    match auth.logout_session(&current.session_token).await {
        Ok(()) | Err(AuthError::SessionExpired(_)) => {}
        Err(other) => {
            return LogoutOutcome::Failure(describe_auth_error(&other));
        }
    }
    if let Err(err) = deps.token_store.clear() {
        return LogoutOutcome::Failure(format!(
            "Server revoke succeeded but local clear failed: {}. \
             Retry once the keystore is reachable.",
            describe_store_error(&err)
        ));
    }
    LogoutOutcome::Success
}

/// Sanity-check an email before hitting the server. We don't try to
/// fully parse RFC 5322 — the server validates definitively; this just
/// catches the trivial empty / no-`@` cases inline so the user sees the
/// error without a network round trip.
pub fn validate_email(email: &str) -> Result<(), String> {
    if email.is_empty() {
        return Err("Enter an email address.".into());
    }
    if !email.contains('@') {
        return Err("Email must contain '@'.".into());
    }
    Ok(())
}

pub fn validate_code(code: &str) -> Result<(), String> {
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return Err("Code must be exactly 6 digits.".into());
    }
    Ok(())
}

pub fn describe_auth_error(err: &AuthError) -> String {
    match err {
        AuthError::InvalidConfiguration(msg) => format!("Configuration error: {msg}"),
        AuthError::Network(msg) => format!("Network error: {msg}. Check your connection."),
        AuthError::InvalidEmail(msg) => format!("Invalid email: {msg}"),
        AuthError::InvalidCode(msg) => {
            format!("Invalid code: {msg}. Request a new code if it has expired.")
        }
        AuthError::SessionExpired(msg) => format!("Session expired: {msg}. Sign in again."),
        AuthError::RateLimited {
            message,
            retry_after_secs,
        } => retry_after_secs.as_ref().map_or_else(
            || format!("Rate limited ({message}). Try again in a moment."),
            |secs| format!("Rate limited ({message}). Retry in {secs}s."),
        ),
        AuthError::BadRequest { code, message } => {
            format!("Request rejected ({code}): {message}")
        }
        AuthError::ServerUnavailable(msg) => format!("Server unavailable: {msg}"),
        AuthError::ServerError { status, message } => {
            format!("Server error ({status}): {message}")
        }
        AuthError::Decode(msg) => format!("Server response was not understood: {msg}"),
    }
}

pub fn describe_store_error(err: &TokenStoreError) -> String {
    match err {
        TokenStoreError::Backend(msg) => format!("keystore backend error: {msg}"),
        TokenStoreError::Serialize(msg) => format!("stored token serialize error: {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dirt_core::auth::{AuthClient, MemoryTokenStore, TokenStore};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ---- Pure validation helpers. ----

    #[test]
    fn validate_email_rejects_empty() {
        assert!(validate_email("").is_err());
    }

    #[test]
    fn validate_email_rejects_missing_at() {
        let err = validate_email("user.example.com").unwrap_err();
        assert!(err.contains("'@'"));
    }

    #[test]
    fn validate_email_accepts_basic_address() {
        validate_email("user@example.com").unwrap();
    }

    #[test]
    fn validate_code_requires_six_digits() {
        for bad in ["", "12345", "1234567", "12345a", "abcdef"] {
            assert!(validate_code(bad).is_err(), "{bad} should be rejected");
        }
        validate_code("000000").unwrap();
        validate_code("123456").unwrap();
    }

    #[test]
    fn describe_auth_error_includes_retry_hint_when_present() {
        let err = AuthError::RateLimited {
            message: "slow down".into(),
            retry_after_secs: Some(45),
        };
        let described = describe_auth_error(&err);
        assert!(described.contains("Retry in 45s"));
    }

    #[test]
    fn describe_auth_error_falls_back_when_no_retry_hint() {
        let err = AuthError::RateLimited {
            message: "slow down".into(),
            retry_after_secs: None,
        };
        let described = describe_auth_error(&err);
        assert!(described.contains("Try again in a moment"));
    }

    #[test]
    fn describe_store_error_distinguishes_variants() {
        assert!(
            describe_store_error(&TokenStoreError::Backend("keystore offline".into()))
                .contains("keystore backend")
        );
        assert!(
            describe_store_error(&TokenStoreError::Serialize("bad json".into()))
                .contains("serialize")
        );
    }

    // ---- Flow integration via wiremock (mirrors desktop). ----

    fn deps_for(server: &MockServer, store: Arc<dyn TokenStore>) -> AuthDeps {
        let auth = AuthClient::new(server.uri()).expect("auth client should build for mock");
        let base_url = auth.base_url().to_string();
        AuthDeps {
            auth_client: Some(Arc::new(auth)),
            token_store: store,
            api_base_url: Some(base_url),
        }
    }

    fn empty_store() -> Arc<MemoryTokenStore> {
        Arc::new(MemoryTokenStore::new())
    }

    /// Happy-path `send_magic_code` → returns the server's `request_id`.
    #[tokio::test(flavor = "current_thread")]
    async fn send_magic_code_returns_request_id_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req-abc",
                "expires_at_ms": 9_999_999i64,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let deps = deps_for(&server, empty_store());
        let req_id = send_magic_code(&deps, "user@example.com").await.unwrap();
        assert_eq!(req_id, "req-abc");
    }

    /// Missing `AuthClient` (no `DIRT_API_BASE_URL`) must surface a
    /// clear "unavailable" message rather than panicking.
    #[tokio::test(flavor = "current_thread")]
    async fn send_magic_code_reports_missing_auth_client() {
        let deps = AuthDeps {
            auth_client: None,
            token_store: empty_store(),
            api_base_url: None,
        };
        let err = send_magic_code(&deps, "user@example.com")
            .await
            .unwrap_err();
        assert!(err.contains("Sign-in is unavailable"));
    }

    /// Happy-path verify → token persisted + Success.
    #[tokio::test(flavor = "current_thread")]
    async fn perform_verify_persists_token_and_returns_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/verify"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "tok-new",
                "session_id": "sid-new",
                "user_id": "uid-1",
                "email": "user@example.com",
                "expires_at_ms": 9_999_999i64,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store = empty_store();
        let deps = deps_for(&server, store.clone());
        let outcome = perform_verify(&deps, "req-abc", "123456").await;
        match outcome {
            LoginOutcome::Success(token, _session) => {
                assert_eq!(token.session_token, "tok-new");
                assert_eq!(token.email, "user@example.com");
                // Persistence side effect:
                let loaded = store.load().unwrap().unwrap();
                assert_eq!(loaded.session_token, "tok-new");
            }
            LoginOutcome::Failure(reason) => panic!("expected success, got: {reason}"),
        }
    }

    /// Server returns `INVALID_CODE` → flow translates into a Failure
    /// outcome with the user-facing copy from `describe_auth_error`.
    #[tokio::test(flavor = "current_thread")]
    async fn perform_verify_returns_failure_on_invalid_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/verify"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "code": "INVALID_CODE",
                    "message": "Code did not match",
                    "cause": "Code did not match",
                    "fix": "Re-enter the code or request a new one.",
                }
            })))
            .mount(&server)
            .await;

        let store = empty_store();
        let deps = deps_for(&server, store.clone());
        let outcome = perform_verify(&deps, "req-abc", "999999").await;
        match outcome {
            LoginOutcome::Failure(msg) => assert!(msg.contains("Invalid code")),
            LoginOutcome::Success(..) => panic!("expected failure"),
        }
        // No persistence side effect on failure:
        assert!(store.load().unwrap().is_none());
    }

    /// Logout reads the *freshest* bearer from the store and POSTs it
    /// to `/v1/auth/logout`. `AuthClient::logout_session` accepts 204
    /// No Content as success; the test mirrors that contract.
    #[tokio::test(flavor = "current_thread")]
    async fn perform_logout_revokes_freshest_stored_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let store = Arc::new(MemoryTokenStore::with_initial(StoredToken {
            session_token: "refreshed-tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 9_999_999,
        }));
        let deps = deps_for(&server, store.clone());

        match perform_logout(&deps).await {
            LogoutOutcome::Success => {}
            LogoutOutcome::Failure(reason) => panic!("expected success, got: {reason}"),
        }
        // Slot must be cleared after successful revoke.
        assert!(store.load().unwrap().is_none());
    }

    /// `SESSION_EXPIRED` on logout means the server already considers
    /// the session dead — still a success from the user's POV; we just
    /// clear the local slot.
    #[tokio::test(flavor = "current_thread")]
    async fn perform_logout_treats_session_expired_as_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
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

        let store = Arc::new(MemoryTokenStore::with_initial(StoredToken {
            session_token: "stale-tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1,
        }));
        let deps = deps_for(&server, store.clone());

        match perform_logout(&deps).await {
            LogoutOutcome::Success => assert!(store.load().unwrap().is_none()),
            LogoutOutcome::Failure(reason) => panic!("expected success, got: {reason}"),
        }
    }

    /// Logout without `auth_client` (CLI / desktop logged in but
    /// mobile's `DIRT_API_BASE_URL` is unset) must refuse to clear the
    /// local slot. A silent wipe would orphan the live server session.
    #[tokio::test(flavor = "current_thread")]
    async fn perform_logout_refuses_when_auth_client_missing() {
        let store = Arc::new(MemoryTokenStore::with_initial(StoredToken {
            session_token: "tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 9_999_999,
        }));
        let deps = AuthDeps {
            auth_client: None,
            token_store: store.clone(),
            api_base_url: None,
        };

        match perform_logout(&deps).await {
            LogoutOutcome::Failure(msg) => {
                assert!(msg.contains("DIRT_API_BASE_URL"));
                // Slot must be preserved so the CLI can still revoke.
                assert!(store.load().unwrap().is_some());
            }
            LogoutOutcome::Success => panic!("expected failure (auth_client missing)"),
        }
    }

    /// Logout on an empty store is a no-op success (nothing to revoke).
    #[tokio::test(flavor = "current_thread")]
    async fn perform_logout_succeeds_when_store_already_empty() {
        let server = MockServer::start().await;
        let deps = deps_for(&server, empty_store());
        match perform_logout(&deps).await {
            LogoutOutcome::Success => {}
            LogoutOutcome::Failure(reason) => panic!("expected success, got: {reason}"),
        }
    }
}
