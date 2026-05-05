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
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::turso::SessionRow;
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

    // Every failure mode collapses to INVALID_CODE on the wire.
    // Distinct error codes (EXPIRED / TOO_MANY_ATTEMPTS) would let a
    // probing attacker tell "this request_id never existed" from "this
    // request_id existed but expired/locked" — which is exactly the
    // signal the catch-all is meant to deny.
    //
    // The repo still distinguishes these for server-side log
    // diagnostics, but the user-facing error is one shape with a fix
    // message that covers all three branches.
    let email = match state
        .repo
        .consume_magic_code(request_id, &code_hash, now_ms)
        .await?
    {
        Ok(email) => email,
        Err(failure) => {
            tracing::debug!(target: "dirt_api::auth", "consume_magic_code failed: {failure:?}");
            return Err(AppError::invalid_code(
                "request_id + code do not match a usable magic code (it may be wrong, expired, or locked after too many attempts)",
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
    // OsRng for auth-critical randomness — explicit about going to the
    // OS CSPRNG rather than relying on `thread_rng`'s seeding being
    // cryptographically secure on every target.
    //
    // `next_u32() % 1_000_000` has a small modulo bias (about 0.024%)
    // since 2^32 isn't a multiple of 1e6. For a 6-digit auth code the
    // bias is well below the 1-in-200,000 floor that the 5-attempt cap
    // already enforces, so it's not worth a rejection-sampling loop.
    let n = OsRng.next_u32() % 1_000_000;
    format!("{n:06}")
}

fn is_six_digit_code(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit())
}

fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
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
    use crate::email::{CapturedSends, EmailSender};
    use crate::turso::TempDb;
    use crate::TursoRepo;

    /// Test fixture. Holds the `TempDb` guard so the scratch DB file is
    /// removed when the test scope exits.
    struct Fixture {
        router: Router,
        state: AppState,
        captured: CapturedSends,
        _temp_db: TempDb,
    }

    async fn build_test_router() -> Fixture {
        let config = AppConfig {
            bind_addr: "127.0.0.1:0".into(),
            turso_database_url: "libsql://unused.test".into(),
            turso_auth_token: "unused".into(),
            server_token: ServerToken(b"unused-32-byte-server-token-abcdef".to_vec()),
        };
        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let (sender, captured) = EmailSender::capture();
        let state = AppState::new(Arc::new(config), temp_db.repo.clone(), Arc::new(sender));
        let router = crate::build_router(state.clone());
        Fixture {
            router,
            state,
            captured,
            _temp_db: temp_db,
        }
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
        let fx = build_test_router().await;

        // Step 1: client requests a code. The capture-mode EmailSender
        // stashes the (email, code) pair for us so we can submit the
        // *real* code that /v1/auth/request minted, instead of seeding
        // a parallel one.
        let resp = fx
            .router
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
        let request_id = body["request_id"].as_str().unwrap().to_string();
        assert!(uuid::Uuid::parse_str(&request_id).is_ok());
        assert!(body.get("expires_at_ms").and_then(Value::as_i64).is_some());

        let captured = fx.captured.lock().unwrap().clone();
        assert_eq!(captured.len(), 1, "expected exactly one captured send");
        let (sent_to, code) = captured.into_iter().next().unwrap();
        assert_eq!(sent_to, "user@example.com");
        assert!(is_six_digit_code(&code), "captured code wasn't 6 digits");

        // Step 2: verify with the actual minted code.
        let resp = fx
            .router
            .clone()
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
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["email"], "user@example.com");
        assert_eq!(body["session_token"].as_str().unwrap().len(), 43);
        assert!(uuid::Uuid::parse_str(body["user_id"].as_str().unwrap()).is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verify_with_wrong_code_returns_invalid_code_and_locks_after_max_attempts() {
        let fx = build_test_router().await;
        let request_id = seed_code(&fx.state, "user@example.com", "111111").await;

        for _ in 0..crate::turso::MAX_CODE_ATTEMPTS {
            let resp = fx
                .router
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

        // Sixth try (with the now-correct code) still fails — the
        // INVALID_CODE catch-all now covers "locked after too many
        // attempts" too, so the attacker can't tell why their code
        // didn't work. The row IS locked: the only way out is a fresh
        // /v1/auth/request.
        let resp = fx
            .router
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
        assert_eq!(body["error"]["code"], "INVALID_CODE");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verify_expired_code_returns_invalid_code() {
        let fx = build_test_router().await;
        // Seed a row with expires_at already past.
        let request_id = uuid::Uuid::now_v7().to_string();
        let code = "222222";
        let code_hash = hash_code(&request_id, code);
        fx.state
            .repo
            .insert_magic_code(&request_id, "user@example.com", &code_hash, 0, 1)
            .await
            .unwrap();

        let resp = fx
            .router
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
        // Expired collapses to INVALID_CODE so an attacker can't
        // distinguish "this request_id never existed" from "it existed
        // but expired".
        assert_eq!(body["error"]["code"], "INVALID_CODE");
    }

    /// Replaying a successfully-consumed code on the same `request_id`
    /// must come back as `INVALID_CODE`. The `consumed_at` guard in the
    /// success-path UPDATE is what enforces this.
    #[tokio::test(flavor = "current_thread")]
    async fn verify_replay_after_success_returns_invalid_code() {
        let fx = build_test_router().await;
        let request_id = seed_code(&fx.state, "user@example.com", "555555").await;

        let body = json!({ "request_id": request_id, "code": "555555" });

        let resp = fx
            .router
            .clone()
            .oneshot(json_request("POST", "/v1/auth/verify", body.clone()).await)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = fx
            .router
            .oneshot(json_request("POST", "/v1/auth/verify", body).await)
            .await
            .unwrap();
        let (status, body) = read_json(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "INVALID_CODE");
    }

    /// Logging in twice with the same email must return the same
    /// `user_id`. Guards the `ON CONFLICT(email)` path on the users
    /// upsert from a regression that minted a fresh user per login.
    #[tokio::test(flavor = "current_thread")]
    async fn upsert_user_by_email_is_idempotent_across_logins() {
        let fx = build_test_router().await;

        let mut user_ids = Vec::new();
        for code in ["666666", "777777"] {
            let request_id = seed_code(&fx.state, "twice@example.com", code).await;
            let resp = fx
                .router
                .clone()
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
            assert_eq!(status, StatusCode::OK);
            user_ids.push(body["user_id"].as_str().unwrap().to_string());
        }

        assert_eq!(user_ids[0], user_ids[1], "same email yielded a new user_id");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refresh_rotates_session_and_revokes_old_token() {
        let fx = build_test_router().await;
        let request_id = seed_code(&fx.state, "user@example.com", "333333").await;

        // verify → first session token
        let resp = fx
            .router
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
        let resp = fx
            .router
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
        let resp = fx
            .router
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
        let resp = fx
            .router
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
        let fx = build_test_router().await;
        let request_id = seed_code(&fx.state, "user@example.com", "444444").await;

        let resp = fx
            .router
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

        let resp = fx
            .router
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

        let resp = fx
            .router
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
        let fx = build_test_router().await;
        let resp = fx
            .router
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
        let fx = build_test_router().await;
        let resp = fx
            .router
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
        let fx = build_test_router().await;
        let request_id = uuid::Uuid::now_v7().to_string();
        let resp = fx
            .router
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
