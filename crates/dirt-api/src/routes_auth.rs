//! Magic-code auth endpoints.
//!
//! Four routes wired under `/v1/auth/*`:
//!
//!   - `POST /v1/auth/request`  — start a login by emailing a code.
//!   - `POST /v1/auth/verify`   — exchange (`request_id`, code) for a session.
//!   - `POST /v1/auth/refresh`  — rotate a session token (revokes the old).
//!   - `POST /v1/auth/logout`   — revoke the current session.
//!
//! The first two are public; rate limiting on `/v1/*` keeps them honest.
//! `refresh` and `logout` consume `Authorization: Bearer <session-token>`
//! and resolve it via `TursoRepo::lookup_session_by_token_hash`.
//!
//! All on-the-wire secrets (codes, session tokens) live only in the
//! response of the route that mints them. The DB stores sha256 hashes.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::turso::{ConsumeFailure, SessionRow};
use crate::AppState;

/// Magic-code lifetime. 15 minutes is long enough to switch tabs, deal
/// with a phone unlock, retype a typo'd code; short enough that a
/// stolen email doesn't sit indefinitely as a usable login.
const CODE_TTL_MS: i64 = 15 * 60 * 1000;

/// Session lifetime. 30 days matches typical "stay signed in" UX. The
/// session row's `last_used_at` rolls forward on every authed request,
/// but `expires_at` only moves on an explicit refresh.
const SESSION_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;

const BEARER_PREFIX_LEN: usize = "Bearer ".len();

// ---- POST /v1/auth/request ----

#[derive(Debug, Deserialize)]
pub struct RequestBody {
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct RequestResponse {
    pub request_id: String,
    pub expires_at_ms: i64,
}

pub async fn request_magic_code(
    State(state): State<AppState>,
    Json(body): Json<RequestBody>,
) -> Result<Json<RequestResponse>, AppError> {
    let email = normalize_email(&body.email)?;

    let request_id = uuid::Uuid::now_v7().to_string();
    let code = generate_six_digit_code();
    let code_hash = hash_code(&request_id, &code);

    let now_ms = now_ms();
    let expires_at_ms = now_ms + CODE_TTL_MS;

    state
        .repo
        .insert_magic_code(&request_id, &email, &code_hash, now_ms, expires_at_ms)
        .await?;

    state.email.send_magic_code(&email, &code).await?;

    Ok(Json(RequestResponse {
        request_id,
        expires_at_ms,
    }))
}

// ---- POST /v1/auth/verify ----

#[derive(Debug, Deserialize)]
pub struct VerifyBody {
    pub request_id: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub session_token: String,
    pub session_id: String,
    pub user_id: String,
    pub email: String,
    pub expires_at_ms: i64,
}

pub async fn verify_magic_code(
    State(state): State<AppState>,
    Json(body): Json<VerifyBody>,
) -> Result<Json<VerifyResponse>, AppError> {
    if !is_six_digit_code(&body.code) {
        return Err(AppError::invalid_code("code must be exactly 6 digits"));
    }
    if uuid::Uuid::parse_str(body.request_id.trim()).is_err() {
        return Err(AppError::invalid_code("request_id is not a valid UUID"));
    }

    let request_id = body.request_id.trim();
    let code_hash = hash_code(request_id, &body.code);
    let now_ms = now_ms();

    let email = match state
        .repo
        .consume_magic_code(request_id, &code_hash, now_ms)
        .await?
    {
        Ok(email) => email,
        Err(ConsumeFailure::InvalidCode) => {
            return Err(AppError::invalid_code(
                "request_id and code do not match an outstanding magic code",
            ));
        }
        Err(ConsumeFailure::Expired) => {
            return Err(AppError::expired_code(
                "this magic code has expired; request a new one",
            ));
        }
        Err(ConsumeFailure::TooManyAttempts) => {
            return Err(AppError::too_many_attempts(
                "this request_id is locked after too many failed attempts; request a new code",
            ));
        }
    };

    let user_id = state.repo.upsert_user_by_email(&email, now_ms).await?;
    let (session_token, session_id, expires_at_ms) = mint_session(&state, &user_id, now_ms).await?;

    Ok(Json(VerifyResponse {
        session_token,
        session_id,
        user_id,
        email,
        expires_at_ms,
    }))
}

// ---- POST /v1/auth/refresh ----

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub session_token: String,
    pub session_id: String,
    pub expires_at_ms: i64,
}

pub async fn refresh_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RefreshResponse>, AppError> {
    let now_ms = now_ms();
    let session = require_authed_session(&state, &headers, now_ms).await?;

    state.repo.revoke_session(&session.id, now_ms).await?;
    let (session_token, session_id, expires_at_ms) =
        mint_session(&state, &session.user_id, now_ms).await?;

    Ok(Json(RefreshResponse {
        session_token,
        session_id,
        expires_at_ms,
    }))
}

// ---- POST /v1/auth/logout ----

pub async fn logout_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let now_ms = now_ms();
    let session = require_authed_session(&state, &headers, now_ms).await?;
    state.repo.revoke_session(&session.id, now_ms).await?;
    Ok((StatusCode::NO_CONTENT, ()).into_response())
}

// ---- helpers ----

async fn mint_session(
    state: &AppState,
    user_id: &str,
    now_ms: i64,
) -> Result<(String, String, i64), AppError> {
    let session_token = generate_session_token();
    let token_hash = sha256_b64url(session_token.as_bytes());
    let expires_at_ms = now_ms + SESSION_TTL_MS;
    let session_id = state
        .repo
        .insert_auth_session(user_id, &token_hash, now_ms, expires_at_ms)
        .await?;
    Ok((session_token, session_id, expires_at_ms))
}

/// Pull a session token off the request and resolve it. Returns
/// `SESSION_EXPIRED` (401) for every "no good session" condition so the
/// client treats them all as "go log in again".
async fn require_authed_session(
    state: &AppState,
    headers: &HeaderMap,
    now_ms: i64,
) -> Result<SessionRow, AppError> {
    let token = extract_bearer(headers)?;
    let token_hash = sha256_b64url(token.as_bytes());
    state
        .repo
        .lookup_session_by_token_hash(&token_hash, now_ms)
        .await?
        .ok_or_else(|| AppError::session_expired("session token is invalid or expired"))
}

fn extract_bearer(headers: &HeaderMap) -> Result<String, AppError> {
    let header_value = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| AppError::session_expired("missing Authorization header"))?;
    let header_str = header_value
        .to_str()
        .map_err(|_| AppError::session_expired("Authorization header is not valid UTF-8"))?;
    if header_str.len() < BEARER_PREFIX_LEN
        || !header_str[..BEARER_PREFIX_LEN].eq_ignore_ascii_case("Bearer ")
    {
        return Err(AppError::session_expired(
            "Authorization header must start with 'Bearer '",
        ));
    }
    Ok(header_str[BEARER_PREFIX_LEN..].to_string())
}

fn normalize_email(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_email("email is empty"));
    }
    // We don't try to be RFC 5322 compliant — the magic-code round-trip
    // is the actual liveness check. Just rule out obvious garbage:
    // - exactly one '@'
    // - non-empty local + domain parts
    // - domain contains a '.'
    if trimmed.bytes().filter(|&b| b == b'@').count() != 1 {
        return Err(AppError::invalid_email(
            "email must contain exactly one '@'",
        ));
    }
    let (local, domain) = trimmed
        .split_once('@')
        .ok_or_else(|| AppError::invalid_email("email must contain '@'"))?;
    if local.is_empty() || domain.is_empty() {
        return Err(AppError::invalid_email(
            "email local and domain parts must be non-empty",
        ));
    }
    if !domain.contains('.') {
        return Err(AppError::invalid_email("email domain must contain a '.'"));
    }
    if trimmed.len() > 254 {
        return Err(AppError::invalid_email(
            "email exceeds 254 characters (RFC 5321 max)",
        ));
    }
    Ok(trimmed.to_ascii_lowercase())
}

fn generate_six_digit_code() -> String {
    let n = rand::Rng::gen_range(&mut rand::thread_rng(), 0..1_000_000);
    format!("{n:06}")
}

fn is_six_digit_code(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit())
}

fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_code(request_id: &str, code: &str) -> String {
    sha256_b64url(format!("{request_id}:{code}").as_bytes())
}

fn sha256_b64url(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    URL_SAFE_NO_PAD.encode(digest)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use super::*;
    use crate::config::{AppConfig, ServerToken};
    use crate::email::EmailSender;
    use crate::TursoRepo;

    async fn build_test_router() -> (Router, AppState) {
        let config = AppConfig {
            bind_addr: "127.0.0.1:0".into(),
            turso_database_url: "libsql://unused.test".into(),
            turso_auth_token: "unused".into(),
            server_token: ServerToken(b"unused-32-byte-server-token-abcdef".to_vec()),
        };
        let repo = Arc::new(TursoRepo::connect_in_memory().await.unwrap());
        let email = Arc::new(EmailSender::log_only());
        let state = AppState::new(Arc::new(config), repo, email);
        let router = crate::build_router(state.clone());
        (router, state)
    }

    // Kept `async` even though the body never awaits, so the call sites
    // can stay in `.oneshot(json_request(...).await,)` form alongside
    // their genuinely-async neighbours without churn.
    #[allow(clippy::unused_async)]
    async fn json_request(method: &str, uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    async fn read_json(resp: axum::response::Response) -> (StatusCode, Value) {
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let value: Value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    /// Seed a known magic code straight via the repo so tests can
    /// exercise `/v1/auth/verify` without parsing log output.
    async fn seed_code(state: &AppState, email: &str, code: &str) -> String {
        let request_id = uuid::Uuid::now_v7().to_string();
        let code_hash = hash_code(&request_id, code);
        let now = now_ms();
        state
            .repo
            .insert_magic_code(&request_id, email, &code_hash, now, now + CODE_TTL_MS)
            .await
            .unwrap();
        request_id
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_then_verify_round_trip() {
        let (router, state) = build_test_router().await;

        // Step 1: client requests a code. Response carries an opaque
        // request_id and an expiry; the actual code is in the email
        // (here: tracing log). We seed via repo for the verify step.
        let resp = router
            .clone()
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/request",
                    json!({ "email": "user@example.com" }),
                )
                .await,
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.get("request_id").and_then(Value::as_str).is_some());
        assert!(body.get("expires_at_ms").and_then(Value::as_i64).is_some());

        // Step 2: seed a known code and verify with it.
        let request_id = seed_code(&state, "user@example.com", "424242").await;
        let resp = router
            .clone()
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": request_id, "code": "424242" }),
                )
                .await,
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["email"], "user@example.com");
        assert!(body["session_token"].as_str().unwrap().len() == 43);
        assert!(uuid::Uuid::parse_str(body["user_id"].as_str().unwrap()).is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verify_with_wrong_code_returns_invalid_code_and_locks_after_max_attempts() {
        let (router, state) = build_test_router().await;
        let request_id = seed_code(&state, "user@example.com", "111111").await;

        for _ in 0..crate::turso::MAX_CODE_ATTEMPTS {
            let resp = router
                .clone()
                .oneshot(
                    json_request(
                        "POST",
                        "/v1/auth/verify",
                        json!({ "request_id": request_id, "code": "999999" }),
                    )
                    .await,
                )
                .await
                .unwrap();
            let (status, body) = read_json(resp).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["code"], "INVALID_CODE");
        }

        // Sixth try (with the now-correct code) fails with TOO_MANY_ATTEMPTS,
        // proving the attempts counter actually locked the row.
        let resp = router
            .clone()
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": request_id, "code": "111111" }),
                )
                .await,
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "TOO_MANY_ATTEMPTS");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verify_expired_code_returns_expired_code() {
        let (router, state) = build_test_router().await;
        // Seed a row with expires_at already past.
        let request_id = uuid::Uuid::now_v7().to_string();
        let code = "222222";
        let code_hash = hash_code(&request_id, code);
        state
            .repo
            .insert_magic_code(&request_id, "user@example.com", &code_hash, 0, 1)
            .await
            .unwrap();

        let resp = router
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": request_id, "code": code }),
                )
                .await,
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "EXPIRED_CODE");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_rotates_session_and_revokes_old_token() {
        let (router, state) = build_test_router().await;
        let request_id = seed_code(&state, "user@example.com", "333333").await;

        // verify → first session token
        let resp = router
            .clone()
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": request_id, "code": "333333" }),
                )
                .await,
            )
            .await
            .unwrap();
        let (_, body) = read_json(resp).await;
        let token_a = body["session_token"].as_str().unwrap().to_string();

        // refresh → second session token
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/refresh")
                    .header("authorization", format!("Bearer {token_a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        let token_b = body["session_token"].as_str().unwrap().to_string();
        assert_ne!(token_a, token_b);

        // The old token should now be SESSION_EXPIRED on logout.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/logout")
                    .header("authorization", format!("Bearer {token_a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "SESSION_EXPIRED");

        // The new token still works (logout it).
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/logout")
                    .header("authorization", format!("Bearer {token_b}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn logout_then_reuse_returns_session_expired() {
        let (router, state) = build_test_router().await;
        let request_id = seed_code(&state, "user@example.com", "444444").await;

        let resp = router
            .clone()
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": request_id, "code": "444444" }),
                )
                .await,
            )
            .await
            .unwrap();
        let (_, body) = read_json(resp).await;
        let token = body["session_token"].as_str().unwrap().to_string();

        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/logout")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/logout")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "SESSION_EXPIRED");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_without_authorization_header_returns_session_expired() {
        let (router, _) = build_test_router().await;
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/refresh")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "SESSION_EXPIRED");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_with_garbage_email_returns_invalid_email() {
        let (router, _) = build_test_router().await;
        let resp = router
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/request",
                    json!({ "email": "not-an-email" }),
                )
                .await,
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "INVALID_EMAIL");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verify_with_malformed_code_returns_invalid_code() {
        let (router, _) = build_test_router().await;
        let request_id = uuid::Uuid::now_v7().to_string();
        let resp = router
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": request_id, "code": "abc" }),
                )
                .await,
            )
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "INVALID_CODE");
    }

    // ---- pure-function tests preserved from before ----

    #[test]
    fn normalize_email_lowercases_and_trims() {
        assert_eq!(
            normalize_email("  Hello@Example.COM ").unwrap(),
            "hello@example.com"
        );
    }

    #[test]
    fn normalize_email_rejects_garbage() {
        assert!(normalize_email("").is_err());
        assert!(normalize_email("no-at-sign").is_err());
        assert!(normalize_email("@nolocal.com").is_err());
        assert!(normalize_email("nodomain@").is_err());
        assert!(normalize_email("two@@signs.com").is_err());
        assert!(normalize_email("nodot@localhost").is_err());
    }

    #[test]
    fn six_digit_code_round_trip() {
        for _ in 0..100 {
            let c = generate_six_digit_code();
            assert!(is_six_digit_code(&c), "rejected own output: {c}");
        }
    }

    #[test]
    fn six_digit_code_validation() {
        assert!(is_six_digit_code("000000"));
        assert!(is_six_digit_code("123456"));
        assert!(!is_six_digit_code("12345"));
        assert!(!is_six_digit_code("1234567"));
        assert!(!is_six_digit_code("12345a"));
        assert!(!is_six_digit_code("      "));
    }

    #[test]
    fn hash_binds_code_to_request_id() {
        // Same code under different request_ids must hash to different
        // values — otherwise a code from one request could authenticate
        // against another.
        let a = hash_code("req-a", "123456");
        let b = hash_code("req-b", "123456");
        assert_ne!(a, b);
    }

    #[test]
    fn session_token_is_43_url_safe_chars() {
        let t = generate_session_token();
        assert_eq!(t.len(), 43);
        assert!(t
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }
}
