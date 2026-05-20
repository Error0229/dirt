//! `dirt auth` — magic-link auth commands.
//!
//! Login / logout go through [`dirt_core::auth::AuthClient`] +
//! [`dirt_core::auth::KeyringTokenStore`]. The `status` command stays
//! on the Phase-1 `DIRT_CLIENT_TOKEN` connectivity probe for now —
//! cutting `status` and `dirt sync` over to the magic-link session is
//! tracked separately (Phase 2.5 was explicitly scoped to
//! login/logout only).

use std::env;
use std::io::{self, BufRead, Write};

use dirt_core::auth::{
    AuthClient, AuthError, KeyringTokenStore, StoredToken, TokenStore, TokenStoreError,
};
use dirt_core::sync::api_client::{ApiClient, ApiClientError};

use crate::cli::AuthCommands;
use crate::config_profiles::{normalize_text_option, CliProfilesConfig};
use crate::error::CliError;

/// Reverse-DNS service identifier the keyring slot is filed under. Shows
/// up verbatim in Keychain Access / Credential Manager / `seahorse` so
/// a user looking for "where does dirt store its session" can find it.
const KEYRING_SERVICE: &str = "dev.dirt.session";

pub async fn run_auth(command: AuthCommands) -> Result<(), CliError> {
    match command {
        AuthCommands::Status => {
            let line = status_line().await?;
            println!("{line}");
            Ok(())
        }
        AuthCommands::Login => {
            let base_url = require_api_base_url()?;
            let client = build_auth_client(&base_url)?;
            let store = build_keyring_store()?;
            login_flow(&client, &store, stdin_read_line).await
        }
        AuthCommands::Logout => {
            let base_url = require_api_base_url()?;
            let client = build_auth_client(&base_url)?;
            let store = build_keyring_store()?;
            logout_flow(&client, &store).await
        }
    }
}

/// Compute the user-facing status line. Returned (not printed) so tests
/// can assert exact strings.
pub async fn status_line() -> Result<String, CliError> {
    let token = normalize_text_option(env::var("DIRT_CLIENT_TOKEN").ok());
    let Some(token) = token else {
        return Ok(
            "offline: DIRT_CLIENT_TOKEN not set — local capture works, sync disabled".to_string(),
        );
    };

    let Some(api_base_url) = resolve_api_base_url()? else {
        return Ok(
            "offline: DIRT_API_BASE_URL not set — local capture works, sync disabled".to_string(),
        );
    };

    let api = match ApiClient::new(api_base_url, token) {
        Ok(api) => api,
        Err(err) => {
            return Ok(format!(
                "offline: auth test failed — invalid configuration: {err}"
            ));
        }
    };

    // The pull endpoint is the cheapest authenticated probe — it
    // doesn't mutate anything and exercises the same bearer middleware
    // production traffic does.
    Ok(match api.pull(None, Some(1)).await {
        Ok(_) => "online: authenticated as solo-user, server ok".to_string(),
        Err(err) => {
            let (cause, fix) = describe_api_error(&err);
            format!("offline: auth test failed — {cause}; {fix}")
        }
    })
}

/// Orchestrate the interactive login flow.
///
/// Lifted out of [`run_auth`] so it can be unit-tested against a
/// `MemoryTokenStore` + wiremock-backed `AuthClient` without touching
/// the OS keyring or real stdin. The `read_line` closure is the seam
/// for the prompts — production passes [`stdin_read_line`], tests pass
/// a `Vec`-backed closure.
async fn login_flow<R>(
    client: &AuthClient,
    store: &dyn TokenStore,
    mut read_line: R,
) -> Result<(), CliError>
where
    R: FnMut(&str) -> io::Result<String>,
{
    let email = read_line("Email: ").map_err(CliError::Io)?;
    if email.is_empty() {
        return Err(CliError::Auth(
            "email must not be empty; nothing was sent".to_string(),
        ));
    }

    let req = client
        .request_magic_code(&email)
        .await
        .map_err(|err| auth_error_to_cli(&err))?;
    println!("Sent a 6-digit code to {email}. Enter it below.");

    let code = read_line("Code: ").map_err(CliError::Io)?;
    if code.is_empty() {
        return Err(CliError::Auth(
            "code must not be empty; run `dirt auth login` again to resend".to_string(),
        ));
    }

    let verify = client
        .verify_magic_code(&req.request_id, &code)
        .await
        .map_err(|err| auth_error_to_cli(&err))?;

    let email_for_message = verify.email.clone();
    let stored: StoredToken = verify.into();
    store
        .save(&stored)
        .map_err(|err| token_store_error_to_cli(&err))?;

    println!("Signed in as {email_for_message}");
    Ok(())
}

/// Orchestrate the logout flow.
///
/// Sequence: load → if empty, print "not signed in" and return; if
/// present, ask the server to revoke the token, then clear the local
/// slot. Clearing only happens after the server-side revoke succeeds
/// (or short-circuits as `SessionExpired` inside `AuthClient`) — a
/// network failure leaves the local token intact so the user can retry
/// without orphaning a still-valid server-side session.
async fn logout_flow(client: &AuthClient, store: &dyn TokenStore) -> Result<(), CliError> {
    let Some(stored) = store.load().map_err(|err| token_store_error_to_cli(&err))? else {
        println!("Not signed in");
        return Ok(());
    };

    client
        .logout_session(&stored.session_token)
        .await
        .map_err(|err| auth_error_to_cli(&err))?;

    store
        .clear()
        .map_err(|err| token_store_error_to_cli(&err))?;
    println!("Logged out");
    Ok(())
}

/// Resolve the dirt-api base URL the same way `status` does (env var
/// wins; profile config is the fallback) but propagate "not configured"
/// as an error instead of silently degrading — login is meaningless
/// without an endpoint to talk to.
fn require_api_base_url() -> Result<String, CliError> {
    resolve_api_base_url()?.ok_or_else(|| {
        CliError::Auth(
            "DIRT_API_BASE_URL not set and no profile is configured; \
             run `dirt config init --api-base-url <URL>` first"
                .to_string(),
        )
    })
}

fn build_auth_client(base_url: &str) -> Result<AuthClient, CliError> {
    AuthClient::new(base_url).map_err(|err| auth_error_to_cli(&err))
}

fn build_keyring_store() -> Result<KeyringTokenStore, CliError> {
    let profile = CliProfilesConfig::load()
        .map_err(CliError::Config)?
        .resolve_profile_name(None);
    Ok(KeyringTokenStore::new(KEYRING_SERVICE, profile))
}

fn resolve_api_base_url() -> Result<Option<String>, CliError> {
    if let Some(url) = normalize_text_option(env::var("DIRT_API_BASE_URL").ok()) {
        return Ok(Some(url));
    }

    let config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(None);
    let Some(profile) = config.profile(&profile_name) else {
        return Ok(None);
    };
    Ok(profile.dirt_api_base_url())
}

fn stdin_read_line(prompt: &str) -> io::Result<String> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    drop(stdout);

    let stdin = io::stdin();
    let mut buf = String::new();
    stdin.lock().read_line(&mut buf)?;
    // `read_line` keeps the trailing newline (and CRLF on Windows);
    // strip leading/trailing whitespace before handing to AuthClient
    // so users don't get `INVALID_EMAIL` for an invisible `\r`.
    Ok(buf.trim().to_string())
}

fn auth_error_to_cli(err: &AuthError) -> CliError {
    let (cause, fix) = describe_auth_error(err);
    CliError::Auth(format!("{cause}; {fix}"))
}

fn token_store_error_to_cli(err: &TokenStoreError) -> CliError {
    let (cause, fix) = match err {
        TokenStoreError::Backend(msg) => (
            format!("token store backend error: {msg}"),
            "ensure the OS keyring is unlocked and accessible, then retry",
        ),
        TokenStoreError::Serialize(msg) => (
            format!("stored token is unreadable: {msg}"),
            "clear the stored session and sign in again with `dirt auth login`",
        ),
    };
    CliError::Auth(format!("{cause}; {fix}"))
}

fn describe_auth_error(err: &AuthError) -> (String, &'static str) {
    match err {
        AuthError::InvalidEmail(msg) => (
            format!("invalid email: {msg}"),
            "double-check the address and try again",
        ),
        AuthError::InvalidCode(msg) => (
            format!("invalid code: {msg}"),
            "the code is wrong, expired, or already used — request a new one with `dirt auth login`",
        ),
        AuthError::SessionExpired(msg) => (
            format!("session expired: {msg}"),
            "sign in again with `dirt auth login`",
        ),
        AuthError::RateLimited {
            message,
            retry_after_secs,
        } => {
            let cause = retry_after_secs.as_ref().map_or_else(
                || format!("rate limited ({message})"),
                |secs| format!("rate limited ({message}); retry after {secs}s"),
            );
            (cause, "wait the cooldown period and try again")
        }
        AuthError::Network(msg) => (
            format!("network error: {msg}"),
            "check connectivity and DIRT_API_BASE_URL, then retry",
        ),
        AuthError::ServerUnavailable(msg) => (
            format!("server unavailable: {msg}"),
            "retry shortly; the dirt-api server may be restarting",
        ),
        AuthError::BadRequest { code, message } => (
            format!("bad request ({code}): {message}"),
            "this should not happen for a normal login — file a bug",
        ),
        AuthError::ServerError { status, message } => (
            format!("server error {status}: {message}"),
            "retry shortly; check server logs",
        ),
        AuthError::Decode(msg) => (
            format!("decode error: {msg}"),
            "client/server contract drift — upgrade the CLI",
        ),
        AuthError::InvalidConfiguration(msg) => (
            format!("invalid configuration: {msg}"),
            "check DIRT_API_BASE_URL and your CLI profile config",
        ),
    }
}

fn describe_api_error(err: &ApiClientError) -> (String, &'static str) {
    match err {
        ApiClientError::Unauthorized(_) => (
            "401 unauthorized".to_string(),
            "rotate DIRT_CLIENT_TOKEN to match the bearer token the server is configured with",
        ),
        ApiClientError::Network(msg) => (
            format!("network error: {msg}"),
            "check DIRT_API_BASE_URL and connectivity",
        ),
        ApiClientError::ServerUnavailable(msg) => (
            format!("server unavailable: {msg}"),
            "retry shortly; check the server's Turso status",
        ),
        ApiClientError::BadRequest { code, message } => (
            format!("bad request ({code}): {message}"),
            "this should not happen for a probe — file a bug",
        ),
        ApiClientError::ServerError { status, message } => (
            format!("server error {status}: {message}"),
            "retry shortly; check server logs",
        ),
        ApiClientError::Decode(msg) => (
            format!("decode error: {msg}"),
            "client/server contract drift — upgrade the CLI",
        ),
        ApiClientError::InvalidConfiguration(msg) => (
            format!("invalid configuration: {msg}"),
            "check DIRT_API_BASE_URL and DIRT_CLIENT_TOKEN",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use dirt_core::auth::MemoryTokenStore;
    use serde_json::json;
    use wiremock::matchers::{bearer_token, body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// Closure-backed reader that pops scripted lines off a `Vec`.
    /// Mirrors the prod `stdin_read_line` shape so the orchestration
    /// can drive it without knowing whether it's wired to real stdin.
    fn scripted_reader(lines: Vec<&'static str>) -> impl FnMut(&str) -> io::Result<String> {
        let lines = RefCell::new(lines.into_iter());
        move |_prompt: &str| {
            lines
                .borrow_mut()
                .next()
                .map(str::to_string)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "scripted reader exhausted")
                })
        }
    }

    fn client_for(server: &MockServer) -> AuthClient {
        AuthClient::new(server.uri()).expect("client should build for mock server")
    }

    #[tokio::test(flavor = "current_thread")]
    #[allow(unsafe_code)]
    async fn status_offline_when_no_client_token() {
        // SAFETY: tests run on current_thread, no concurrent reader.
        unsafe {
            std::env::remove_var("DIRT_CLIENT_TOKEN");
        }
        let line = status_line().await.unwrap();
        assert_eq!(
            line,
            "offline: DIRT_CLIENT_TOKEN not set — local capture works, sync disabled"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_persists_token_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .and(body_json(json!({ "email": "user@example.com" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req-abc",
                "expires_at_ms": 1_700_000_000_000_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/verify"))
            .and(body_json(json!({
                "request_id": "req-abc",
                "code": "123456",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "tok-live",
                "session_id": "sess-1",
                "user_id": "uid-1",
                "email": "user@example.com",
                "expires_at_ms": 1_800_000_000_000_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec!["user@example.com", "123456"]);

        login_flow(&client, &store, reader).await.unwrap();

        let token = store.load().unwrap().expect("token must be persisted");
        assert_eq!(token.session_token, "tok-live");
        assert_eq!(token.session_id, "sess-1");
        assert_eq!(token.user_id, "uid-1");
        assert_eq!(token.email, "user@example.com");
        assert_eq!(token.expires_at_ms, 1_800_000_000_000_i64);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_rejects_empty_email_before_calling_server() {
        // Server mounts no mocks; if the flow tries to hit /request the
        // MockServer panics on the unexpected route.
        let server = MockServer::start().await;
        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec![""]);

        let err = login_flow(&client, &store, reader).await.unwrap_err();
        match err {
            CliError::Auth(msg) => assert!(msg.contains("email must not be empty")),
            other => panic!("expected Auth error, got {other:?}"),
        }
        assert!(store.load().unwrap().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_surfaces_invalid_code_and_leaves_store_empty() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req-1",
                "expires_at_ms": 1_700_000_000_000_i64,
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/verify"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "code": "INVALID_CODE",
                    "message": "wrong code",
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec!["user@example.com", "000000"]);

        let err = login_flow(&client, &store, reader).await.unwrap_err();
        match err {
            CliError::Auth(msg) => {
                assert!(msg.contains("invalid code"), "got {msg}");
                assert!(msg.contains("request a new one"), "got {msg}");
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
        assert!(store.load().unwrap().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_flow_surfaces_rate_limit_with_retry_hint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/request"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "error": {
                    "code": "RATE_LIMITED",
                    "message": "per-email cooldown",
                    "retry_after_secs": 42,
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        let reader = scripted_reader(vec!["user@example.com"]);

        let err = login_flow(&client, &store, reader).await.unwrap_err();
        match err {
            CliError::Auth(msg) => {
                assert!(msg.contains("rate limited"), "got {msg}");
                assert!(msg.contains("42"), "retry hint missing: {msg}");
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_with_empty_store_is_noop() {
        let server = MockServer::start().await;
        // No mocks: if logout_flow hits the network the MockServer will panic.
        let client = client_for(&server);
        let store = MemoryTokenStore::new();
        logout_flow(&client, &store).await.unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_clears_store_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .and(bearer_token("tok-live"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let store = MemoryTokenStore::with_initial(StoredToken {
            session_token: "tok-live".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        });
        let client = client_for(&server);

        logout_flow(&client, &store).await.unwrap();
        assert!(store.load().unwrap().is_none());
    }

    /// `AuthClient` turns a 401-SESSION_EXPIRED into `Ok(())` because
    /// the server intent ("make this token dead") is satisfied. The
    /// local store should still be cleared so the user isn't left
    /// with a dangling dead token.
    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_clears_store_when_server_reports_session_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "already revoked",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store = MemoryTokenStore::with_initial(StoredToken {
            session_token: "tok-dead".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        });
        let client = client_for(&server);

        logout_flow(&client, &store).await.unwrap();
        assert!(store.load().unwrap().is_none());
    }

    /// A real network/server failure during logout must NOT clear the
    /// local store. The server still believes the session is alive, so
    /// dropping the local token would orphan it (and a retry could
    /// then succeed once connectivity is restored).
    #[tokio::test(flavor = "current_thread")]
    async fn logout_flow_preserves_store_on_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "error": {
                    "code": "INTERNAL",
                    "message": "boom",
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store = MemoryTokenStore::with_initial(StoredToken {
            session_token: "tok-live".into(),
            session_id: "sess-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        });
        let client = client_for(&server);

        let err = logout_flow(&client, &store).await.unwrap_err();
        match err {
            CliError::Auth(msg) => assert!(msg.contains("server error 500"), "got {msg}"),
            other => panic!("expected Auth error, got {other:?}"),
        }
        // Token must survive the failed revoke so the user can retry.
        assert!(store.load().unwrap().is_some());
    }

    #[test]
    fn describe_auth_error_includes_retry_hint_when_present() {
        let err = AuthError::RateLimited {
            message: "per-email cooldown".into(),
            retry_after_secs: Some(60),
        };
        let (cause, _fix) = describe_auth_error(&err);
        assert!(cause.contains("60"), "{cause}");
    }

    #[test]
    fn describe_auth_error_omits_retry_hint_when_absent() {
        let err = AuthError::RateLimited {
            message: "per-email cooldown".into(),
            retry_after_secs: None,
        };
        let (cause, _fix) = describe_auth_error(&err);
        assert!(!cause.contains("retry after"), "{cause}");
    }
}
