//! HTTP client for the four magic-link auth endpoints on `dirt-api`.
//!
//! Mirrors `dirt_api::routes_auth`:
//!
//!   - `POST /v1/auth/request` — start a login by emailing a 6-digit code.
//!   - `POST /v1/auth/verify`  — exchange (`request_id`, code) for a session.
//!   - `POST /v1/auth/refresh` — rotate a session token (revokes the old).
//!   - `POST /v1/auth/logout`  — revoke the current session.
//!
//! Wire types are re-declared here instead of being re-exported from
//! `dirt-api` to avoid a circular crate dependency and to make the
//! client-side contract explicit on its own.
//!
//! Errors are bucketed so each downstream consumer (`dirt-cli`,
//! `dirt-desktop`, `dirt-mobile`) can branch on intent without parsing
//! HTTP status codes:
//!
//!   - [`AuthError::InvalidEmail`] / [`AuthError::InvalidCode`] — show
//!     the user a field-level message; do not retry verbatim.
//!   - [`AuthError::SessionExpired`] — the caller must go back through
//!     the magic-code flow.
//!   - [`AuthError::RateLimited`] — honour the `retry_after_secs` hint
//!     before retrying; surface it in the UI so a flood of clicks
//!     doesn't repeat the call.
//!   - [`AuthError::Network`] / [`AuthError::ServerUnavailable`] —
//!     transient; safe to retry with backoff.

use std::fmt;
use std::time::Duration;

use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::util::{is_http_url, normalize_text_option};

const REQUEST_PATH: &str = "/v1/auth/request";
const VERIFY_PATH: &str = "/v1/auth/verify";
const REFRESH_PATH: &str = "/v1/auth/refresh";
const LOGOUT_PATH: &str = "/v1/auth/logout";

/// Per-request timeout for every endpoint on `AuthClient`.
///
/// 30 s is comfortably longer than any expected dirt-api response
/// (the slowest is `/v1/auth/request` which does a Resend HTTPS round
/// trip — capped server-side at 10 s) and short enough that a stalled
/// peer (half-open TCP after a failover, intercepting proxy that
/// accepts the SYN and drops the rest) does not hang the auth UI / CLI
/// indefinitely. Without this, `reqwest::Client::new()` has *no*
/// per-request timeout and the future awaits forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Errors returned by the auth API client.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Base URL was missing or malformed at construction time.
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),
    /// The request never reached the server (DNS, TLS, connection refused,
    /// timeout, etc.). Safe to retry with backoff.
    #[error("network error: {0}")]
    Network(String),
    /// Server replied 400 `INVALID_EMAIL`. The submitted email is
    /// unparseable — surface in the UI on the email field.
    #[error("invalid email: {0}")]
    InvalidEmail(String),
    /// Server replied 400 `INVALID_CODE`. The submitted `request_id`
    /// and code did not match a usable magic code (wrong, expired,
    /// already consumed, or attempt-locked). The server deliberately
    /// collapses these four conditions into one signal so probing for
    /// live `request_id`s does not work.
    #[error("invalid code: {0}")]
    InvalidCode(String),
    /// Server replied 401 `SESSION_EXPIRED`. The session token on this
    /// request is missing, malformed, revoked, or past `expires_at`.
    /// The caller must restart the magic-code flow.
    #[error("session expired: {0}")]
    SessionExpired(String),
    /// Server replied 429 `RATE_LIMITED`. The caller hit the per-email
    /// cooldown (or a future global limiter). `retry_after_secs` is
    /// taken from the response body and mirrors the standard
    /// `Retry-After` header.
    #[error("rate limited: retry after {retry_after_secs}s — {message}")]
    RateLimited {
        message: String,
        retry_after_secs: u64,
    },
    /// Server replied 400 with a code other than `INVALID_EMAIL` /
    /// `INVALID_CODE`. `code` carries the dirt-api-specific error code
    /// so callers can distinguish e.g. `BAD_REQUEST` without re-parsing.
    #[error("bad request ({code}): {message}")]
    BadRequest { code: String, message: String },
    /// Server replied 503 (usually Turso reachability). Safe to retry.
    #[error("server unavailable: {0}")]
    ServerUnavailable(String),
    /// Any other non-2xx response. Includes the raw status so callers
    /// can distinguish 5xx server bugs from unexpected codes.
    #[error("server error ({status}): {message}")]
    ServerError { status: u16, message: String },
    /// Response body could not be decoded against the expected schema.
    /// Almost always a server/client contract drift.
    #[error("decode error: {0}")]
    Decode(String),
}

pub type AuthResult<T> = Result<T, AuthError>;

// ---- Wire types: mirror `dirt_api::routes_auth` exactly. ----

#[derive(Debug, Serialize)]
struct RequestBody<'a> {
    email: &'a str,
}

/// Successful response from `POST /v1/auth/request`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RequestResponse {
    pub request_id: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Serialize)]
struct VerifyBody<'a> {
    request_id: &'a str,
    code: &'a str,
}

/// Successful response from `POST /v1/auth/verify`.
///
/// `session_token` is the bearer credential subsequent authed requests
/// must carry; the other fields are caller-facing identity (`email`,
/// `user_id`) and bookkeeping (`session_id`, `expires_at_ms`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VerifyResponse {
    pub session_token: String,
    pub session_id: String,
    pub user_id: String,
    pub email: String,
    pub expires_at_ms: i64,
}

/// Successful response from `POST /v1/auth/refresh`. Only the rotating
/// fields are returned — the caller is expected to preserve `user_id`
/// and `email` from the previous `VerifyResponse` / stored token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RefreshResponse {
    pub session_token: String,
    pub session_id: String,
    pub expires_at_ms: i64,
}

// ---- Server error envelope (matches `dirt_api::error::ErrorEnvelope`). ----

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
    #[serde(default)]
    retry_after_secs: Option<u64>,
}

// ---- Client ----

/// HTTP client bound to the dirt-api base URL.
///
/// Holds no credentials — anonymous endpoints (`request`/`verify`)
/// take no auth, and bearer endpoints (`refresh`/`logout`) take the
/// session token as a method parameter. Pairs with a
/// [`TokenStore`](super::TokenStore) on the caller side to resolve
/// "which token do I send?" from persistent storage.
#[derive(Clone)]
pub struct AuthClient {
    base_url: String,
    http: Client,
}

impl fmt::Debug for AuthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl AuthClient {
    /// Build a client bound to `base_url`. Trims trailing slashes so
    /// callers can pass either `https://host` or `https://host/`.
    /// Rejects empty / non-http(s) URLs and non-loopback plaintext
    /// HTTP loudly; silent fallback would mask a misconfigured client
    /// as "offline" or leak the session token over plaintext.
    pub fn new(base_url: impl Into<String>) -> AuthResult<Self> {
        let base_url = normalize_base_url(base_url.into())?;
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|err| {
                AuthError::InvalidConfiguration(format!("failed to build HTTP client: {err}"))
            })?;
        Ok(Self { base_url, http })
    }

    /// Expose the normalized base URL for logging/diagnostics.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `POST /v1/auth/request`. Server emails the code and returns the
    /// `request_id` the caller must echo back on `verify_magic_code`.
    pub async fn request_magic_code(&self, email: &str) -> AuthResult<RequestResponse> {
        let url = format!("{}{REQUEST_PATH}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&RequestBody { email })
            .send()
            .await
            .map_err(|err| network_error(&err))?;
        parse_response::<RequestResponse>(resp).await
    }

    /// `POST /v1/auth/verify`. On success, returns a fresh
    /// `session_token` plus user identity. On wrong/expired/locked
    /// codes, returns [`AuthError::InvalidCode`].
    pub async fn verify_magic_code(
        &self,
        request_id: &str,
        code: &str,
    ) -> AuthResult<VerifyResponse> {
        let url = format!("{}{VERIFY_PATH}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&VerifyBody { request_id, code })
            .send()
            .await
            .map_err(|err| network_error(&err))?;
        parse_response::<VerifyResponse>(resp).await
    }

    /// `POST /v1/auth/refresh`. Sends `session_token` as the bearer,
    /// receives a fresh one; the old token is revoked server-side.
    /// On 401 returns [`AuthError::SessionExpired`] — the caller must
    /// restart the magic-code flow.
    pub async fn refresh_session(&self, session_token: &str) -> AuthResult<RefreshResponse> {
        let url = format!("{}{REFRESH_PATH}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(session_token)
            .send()
            .await
            .map_err(|err| network_error(&err))?;
        parse_response::<RefreshResponse>(resp).await
    }

    /// `POST /v1/auth/logout`. Returns `Ok(())` on 204.
    ///
    /// A 401 here is also reported as success **only** when the body's
    /// `error.code` is `SESSION_EXPIRED` — that's the dirt-api signal
    /// for "this token was already invalid", which satisfies the
    /// caller's intent ("make this token dead"). Any other 401
    /// (a future `MISSING_TOKEN`, a proxy-injected unauthorised,
    /// a body we can't parse) propagates as a real `SessionExpired`
    /// error so the caller can decide what to do — silent success on
    /// any-old-401 would let a server-side regression in the auth
    /// path quietly leave tokens un-revoked.
    pub async fn logout_session(&self, session_token: &str) -> AuthResult<()> {
        let url = format!("{}{LOGOUT_PATH}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(session_token)
            .send()
            .await
            .map_err(|err| network_error(&err))?;
        let status = resp.status();
        if status == StatusCode::NO_CONTENT {
            return Ok(());
        }
        if status == StatusCode::UNAUTHORIZED {
            let body = resp.text().await.unwrap_or_default();
            if let Ok(env) = serde_json::from_str::<ErrorEnvelope>(&body) {
                if env.error.code == "SESSION_EXPIRED" {
                    return Ok(());
                }
                return Err(AuthError::SessionExpired(env.error.message));
            }
            return Err(AuthError::SessionExpired(body));
        }
        Err(parse_error_from_status(resp, status).await)
    }
}

// ---- Helpers ----

async fn parse_response<T: serde::de::DeserializeOwned>(resp: Response) -> AuthResult<T> {
    let status = resp.status();
    if status.is_success() {
        return resp
            .json::<T>()
            .await
            .map_err(|err| AuthError::Decode(err.to_string()));
    }
    Err(parse_error_from_status(resp, status).await)
}

async fn parse_error_from_status(resp: Response, status: StatusCode) -> AuthError {
    let body = resp.text().await.unwrap_or_default();
    let (code, message, retry_after_secs) = serde_json::from_str::<ErrorEnvelope>(&body)
        .map_or_else(
            |_| (String::new(), body.clone(), None),
            |env| {
                (
                    env.error.code,
                    env.error.message,
                    env.error.retry_after_secs,
                )
            },
        );

    match status {
        StatusCode::UNAUTHORIZED => AuthError::SessionExpired(message),
        StatusCode::BAD_REQUEST => match code.as_str() {
            "INVALID_EMAIL" => AuthError::InvalidEmail(message),
            "INVALID_CODE" => AuthError::InvalidCode(message),
            _ => AuthError::BadRequest { code, message },
        },
        // The server always sets `retry_after_secs` on 429; if a future
        // upstream proxy injects a 429 without it, fall back to 0 so the
        // caller still gets a typed RateLimited and can default-backoff.
        StatusCode::TOO_MANY_REQUESTS => AuthError::RateLimited {
            message,
            retry_after_secs: retry_after_secs.unwrap_or(0),
        },
        StatusCode::SERVICE_UNAVAILABLE => AuthError::ServerUnavailable(message),
        other => AuthError::ServerError {
            status: other.as_u16(),
            message,
        },
    }
}

fn network_error(err: &reqwest::Error) -> AuthError {
    AuthError::Network(err.to_string())
}

fn normalize_base_url(raw: String) -> AuthResult<String> {
    let normalized = normalize_text_option(Some(raw)).ok_or_else(|| {
        AuthError::InvalidConfiguration("DIRT_API_BASE_URL must not be empty".into())
    })?;
    if !is_http_url(&normalized) {
        return Err(AuthError::InvalidConfiguration(
            "DIRT_API_BASE_URL must start with http:// or https://".into(),
        ));
    }
    if normalized.starts_with("http://") {
        // The session token is the only credential, so plaintext HTTP
        // means anyone on the path can read and replay it. We previously
        // emitted a `tracing::warn!` and proceeded — but a default-level
        // log filter in a release binary swallows warnings silently, so
        // an operator who sets `DIRT_API_BASE_URL=http://api.dirt.dev`
        // would ship bearer tokens over plaintext with no visible
        // signal. Reject loudly instead; the loopback allowlist keeps
        // local dev (Tauri dev server, Android emulator) working.
        if !is_loopback_http_url(&normalized) {
            return Err(AuthError::InvalidConfiguration(
                "DIRT_API_BASE_URL uses plain HTTP for a non-loopback host — \
                 session tokens would be sent in plaintext. Use https:// for \
                 any address other than localhost / 127.0.0.1 / ::1 / 10.0.2.2."
                    .into(),
            ));
        }
    }
    Ok(normalized.trim_end_matches('/').to_string())
}

/// True if `url` is `http://` with a loopback host. Permitted hosts:
///
/// - `127.0.0.1` / `localhost` / `::1` — desktop and CLI local dev.
/// - `10.0.2.2`                        — the Android emulator's
///   loopback-to-host bridge.
///
/// Anything else gets rejected with a configuration error so a
/// production misconfiguration can't silently transmit session tokens
/// in plaintext. Port numbers and trailing paths are tolerated.
///
/// We parse via `reqwest::Url` (the same URL type reqwest will use
/// internally for the actual request) so the loopback check and the
/// transport agree on what the "host" is — no chance of a clever
/// `http://localhost@evil.com/` style mismatch sneaking past.
fn is_loopback_http_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "http" {
        return false;
    }
    // `url::Url::host_str()` returns IPv6 literals wrapped in `[..]`
    // (e.g. `[::1]`) while IPv4 / hostnames come back bare. Accept
    // both forms of the IPv6 loopback so the brackets-or-not
    // distinction isn't a portability landmine for callers.
    matches!(
        parsed.host_str(),
        Some("127.0.0.1" | "localhost" | "::1" | "[::1]" | "10.0.2.2")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{bearer_token, body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_SESSION_TOKEN: &str = "MfWzYZQzfNvbg1bZQwbg__test-session-token-43c";

    fn client_for(server: &MockServer) -> AuthClient {
        AuthClient::new(server.uri()).expect("client should build for mock server")
    }

    // ---- Constructor / config ----

    #[test]
    fn new_rejects_empty_base_url() {
        let err = AuthClient::new("").unwrap_err();
        assert!(matches!(err, AuthError::InvalidConfiguration(_)));
    }

    #[test]
    fn new_rejects_base_url_without_scheme() {
        let err = AuthClient::new("dirt-api.vercel.app").unwrap_err();
        assert!(matches!(err, AuthError::InvalidConfiguration(_)));
    }

    #[test]
    fn new_trims_trailing_slash() {
        let client = AuthClient::new("https://example.com/").unwrap();
        assert_eq!(client.base_url(), "https://example.com");
    }

    /// Plaintext HTTP for a non-loopback host must be rejected — a
    /// silent warn-and-proceed was the old behaviour, but a default
    /// log filter in release builds swallows warnings, and a
    /// misconfigured production deploy would silently transmit
    /// session tokens in plaintext.
    #[test]
    fn new_rejects_plain_http_for_non_loopback_host() {
        let err = AuthClient::new("http://api.dirt.dev").unwrap_err();
        match err {
            AuthError::InvalidConfiguration(msg) => assert!(
                msg.contains("plaintext"),
                "rejection should mention plaintext: {msg}"
            ),
            other => panic!("expected InvalidConfiguration, got {other:?}"),
        }
        assert!(AuthClient::new("http://example.com:8080").is_err());
    }

    /// Loopback http:// must still build — local dev (Tauri dev
    /// server, Android emulator's 10.0.2.2 forward) depends on it.
    #[test]
    fn new_accepts_plain_http_for_loopback_hosts() {
        for url in [
            "http://127.0.0.1:8080",
            "http://localhost:8080",
            "http://localhost",
            "http://[::1]:8080",
            "http://10.0.2.2:8080",
        ] {
            AuthClient::new(url)
                .unwrap_or_else(|err| panic!("expected loopback {url} to build, got {err:?}"));
        }
    }

    #[test]
    fn debug_does_not_leak_internals() {
        let client = AuthClient::new("https://example.com").unwrap();
        let rendered = format!("{client:?}");
        assert!(rendered.contains("https://example.com"));
        // Tokens are never carried in the AuthClient, but make sure the
        // Debug impl stays the typed `finish_non_exhaustive` shape so a
        // future field doesn't accidentally leak.
        assert!(rendered.starts_with("AuthClient"));
    }

    // ---- Happy paths ----

    #[tokio::test]
    async fn request_magic_code_posts_email_and_decodes_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REQUEST_PATH))
            .and(header("content-type", "application/json"))
            .and(body_json(json!({ "email": "user@example.com" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "01932aaa-0000-7000-8000-000000000abc",
                "expires_at_ms": 9_999,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let resp = client.request_magic_code("user@example.com").await.unwrap();
        assert_eq!(resp.request_id, "01932aaa-0000-7000-8000-000000000abc");
        assert_eq!(resp.expires_at_ms, 9_999);
    }

    #[tokio::test]
    async fn verify_magic_code_posts_body_and_decodes_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(VERIFY_PATH))
            .and(body_json(json!({
                "request_id": "01932aaa-0000-7000-8000-000000000abc",
                "code": "123456",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": TEST_SESSION_TOKEN,
                "session_id": "sess-1",
                "user_id": "01932aaa-0000-7000-8000-0000000000ff",
                "email": "user@example.com",
                "expires_at_ms": 1_234_567,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let resp = client
            .verify_magic_code("01932aaa-0000-7000-8000-000000000abc", "123456")
            .await
            .unwrap();
        assert_eq!(resp.session_token, TEST_SESSION_TOKEN);
        assert_eq!(resp.email, "user@example.com");
        assert_eq!(resp.expires_at_ms, 1_234_567);
    }

    #[tokio::test]
    async fn refresh_session_sends_bearer_and_decodes_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REFRESH_PATH))
            .and(bearer_token(TEST_SESSION_TOKEN))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "new-token",
                "session_id": "sess-2",
                "expires_at_ms": 99_999,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let resp = client.refresh_session(TEST_SESSION_TOKEN).await.unwrap();
        assert_eq!(resp.session_token, "new-token");
        assert_eq!(resp.session_id, "sess-2");
        assert_eq!(resp.expires_at_ms, 99_999);
    }

    #[tokio::test]
    async fn logout_session_sends_bearer_and_succeeds_on_204() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(LOGOUT_PATH))
            .and(bearer_token(TEST_SESSION_TOKEN))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        client.logout_session(TEST_SESSION_TOKEN).await.unwrap();
    }

    /// 401 with `SESSION_EXPIRED` on logout is "already revoked" — we
    /// treat that as success because the caller's intent ("make this
    /// token dead") is satisfied. Without this branch every logout
    /// call site would have to match-and-discard the error.
    #[tokio::test]
    async fn logout_session_treats_session_expired_401_as_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(LOGOUT_PATH))
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

        let client = client_for(&server);
        client
            .logout_session("already-revoked-token")
            .await
            .expect("logout should swallow SESSION_EXPIRED 401");
    }

    /// Only `SESSION_EXPIRED` should be swallowed — any other 401 code
    /// (a future `MISSING_TOKEN`, a proxy-injected 401, etc.) means
    /// the server may not have actually revoked the token, so it must
    /// propagate so the caller can decide what to do.
    #[tokio::test]
    async fn logout_session_propagates_non_session_expired_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(LOGOUT_PATH))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "MISSING_TOKEN",
                    "message": "Authorization header was malformed",
                    "cause": "Authorization header was malformed",
                    "fix": "Include 'Authorization: Bearer <session-token>'.",
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.logout_session("malformed").await.unwrap_err();
        match err {
            AuthError::SessionExpired(msg) => assert!(msg.contains("Authorization")),
            other => {
                panic!("expected SessionExpired (preserving the unswallowed 401), got {other:?}")
            }
        }
    }

    /// A 401 with no JSON envelope (proxy-injected, etc.) also must
    /// not be swallowed: we can't confirm the server-side revoke.
    #[tokio::test]
    async fn logout_session_propagates_401_with_unparseable_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(LOGOUT_PATH))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.logout_session("whatever").await.unwrap_err();
        assert!(
            matches!(err, AuthError::SessionExpired(msg) if msg.contains("unauthorized")),
            "non-envelope 401 must surface as SessionExpired, not Ok(())"
        );
    }

    // ---- Error mapping ----

    #[tokio::test]
    async fn request_invalid_email_maps_to_invalid_email() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REQUEST_PATH))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "code": "INVALID_EMAIL",
                    "message": "email must contain '@'",
                    "cause": "email must contain '@'",
                    "fix": "Send a syntactically valid email address (RFC-5322-ish)."
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.request_magic_code("not-an-email").await.unwrap_err();
        match err {
            AuthError::InvalidEmail(msg) => assert!(msg.contains("'@'")),
            other => panic!("expected InvalidEmail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn verify_invalid_code_maps_to_invalid_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(VERIFY_PATH))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "code": "INVALID_CODE",
                    "message": "request_id + code do not match a usable magic code",
                    "cause": "request_id + code do not match a usable magic code",
                    "fix": "Re-check the code; if it keeps failing, request a new one."
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client
            .verify_magic_code("req-id", "000000")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCode(_)));
    }

    #[tokio::test]
    async fn refresh_401_maps_to_session_expired() {
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

        let client = client_for(&server);
        let err = client.refresh_session("dead-token").await.unwrap_err();
        match err {
            AuthError::SessionExpired(msg) => assert!(msg.contains("invalid")),
            other => panic!("expected SessionExpired, got {other:?}"),
        }
    }

    /// 429 must preserve `retry_after_secs` so the UI can show a
    /// countdown / disable the resend button for the right interval.
    #[tokio::test]
    async fn request_rate_limited_preserves_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REQUEST_PATH))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "error": {
                    "code": "RATE_LIMITED",
                    "message": "a magic code was already sent to this email recently",
                    "cause": "a magic code was already sent to this email recently",
                    "fix": "Slow down requests; honour the retry_after_secs hint before retrying.",
                    "retry_after_secs": 42,
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client
            .request_magic_code("flood@example.com")
            .await
            .unwrap_err();
        match err {
            AuthError::RateLimited {
                retry_after_secs,
                message,
            } => {
                assert_eq!(retry_after_secs, 42);
                assert!(message.contains("recently"));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn bad_request_other_code_preserves_server_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(VERIFY_PATH))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "code": "BAD_REQUEST",
                    "message": "request body was not valid JSON",
                    "cause": "request body was not valid JSON",
                    "fix": "Fix the request body or parameters and retry."
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.verify_magic_code("x", "y").await.unwrap_err();
        match err {
            AuthError::BadRequest { code, message } => {
                assert_eq!(code, "BAD_REQUEST");
                assert!(message.contains("JSON"));
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn service_unavailable_maps_to_server_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(VERIFY_PATH))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({
                "error": {
                    "code": "TURSO_UNREACHABLE",
                    "message": "connection refused",
                    "cause": "connection refused",
                    "fix": "Retry shortly."
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.verify_magic_code("x", "y").await.unwrap_err();
        assert!(matches!(err, AuthError::ServerUnavailable(_)));
    }

    #[tokio::test]
    async fn unexpected_5xx_maps_to_server_error_with_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REQUEST_PATH))
            .respond_with(ResponseTemplate::new(500).set_body_string("nginx went sideways"))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.request_magic_code("x@y.z").await.unwrap_err();
        match err {
            AuthError::ServerError { status, message } => {
                assert_eq!(status, 500);
                assert!(message.contains("nginx"));
            }
            other => panic!("expected ServerError(500), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn network_error_when_server_unreachable() {
        // Port 1 is reserved (TCP multiplexer); connection is refused
        // instantly on every platform we care about, making this fast.
        let client = AuthClient::new("http://127.0.0.1:1").unwrap();
        let err = client.request_magic_code("x@y.z").await.unwrap_err();
        assert!(matches!(err, AuthError::Network(_)));
    }

    /// A 200 response with a body that doesn't deserialize against
    /// the typed response shape must classify as `AuthError::Decode`
    /// — that's the "server/client contract drift" arm, separate
    /// from the HTTP-error variants. Previously uncovered.
    #[tokio::test]
    async fn non_json_200_maps_to_decode_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REQUEST_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.request_magic_code("x@y.z").await.unwrap_err();
        assert!(
            matches!(err, AuthError::Decode(_)),
            "expected Decode for non-JSON 200, got {err:?}"
        );
    }
}
