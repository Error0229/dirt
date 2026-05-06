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
use crate::turso::{InsertMagicCodeOutcome, SessionRow};
use crate::AppState;

/// Magic-code lifetime. 15 minutes is long enough to switch tabs, deal
/// with a phone unlock, retype a typo'd code; short enough that a
/// stolen email doesn't sit indefinitely as a usable login.
const CODE_TTL_MS: i64 = 15 * 60 * 1000;

/// Per-email cooldown between successful `/v1/auth/request` calls.
/// Stops an attacker from email-flooding a victim's inbox once Resend
/// is wired in P2.3. The cooldown is shorter than the code TTL because
/// a legit user who fat-fingered their email needs to be able to
/// re-request soon, and the existing 5-attempt cap already covers the
/// typed-code-wrong case without a fresh send.
const REQUEST_COOLDOWN_MS: i64 = 60 * 1000;

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
    let now_ms = now_ms();
    let request_id = uuid::Uuid::now_v7().to_string();
    let code = generate_six_digit_code();
    let code_hash = hash_code(&request_id, &code);
    let expires_at_ms = now_ms + CODE_TTL_MS;

    // Per-email cooldown gate is folded into the repo method so the
    // SELECT, reap, and INSERT all run inside a single
    // `BEGIN IMMEDIATE` / `COMMIT` transaction — without that, two
    // concurrent `/v1/auth/request` for the same email can both pass
    // a separate-conn cooldown SELECT, both insert, and (post-Resend)
    // both email the victim. Locked codes (5+ failed attempts) do not
    // count as "live" here, so a user who exhausted their attempts can
    // immediately request a fresh code.
    //
    // NOTE: the limiter behind this is per-process. On a multi-instance
    // deployment (e.g. Vercel scaling out), a determined attacker could
    // amplify across instances. Solo-phase deploy is single-process per
    // invocation; revisit when we move beyond that.
    let outcome = state
        .repo
        .try_insert_magic_code_with_cooldown(
            &request_id,
            &email,
            &code_hash,
            now_ms,
            expires_at_ms,
            REQUEST_COOLDOWN_MS,
        )
        .await?;

    match outcome {
        InsertMagicCodeOutcome::Inserted => {}
        InsertMagicCodeOutcome::OnCooldown { retry_after_ms } => {
            // Round up to the nearest whole second — `(ms + 999) / 1000`
            // — so a sub-1s wait doesn't round to 0 and look like
            // "retry immediately".
            let retry_after_secs = u64::try_from((retry_after_ms + 999) / 1000)
                .unwrap_or(1)
                .max(1);
            return Err(AppError::rate_limited(
                "a magic code was already sent to this email recently; wait before requesting another",
                retry_after_secs,
            ));
        }
    }

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

    // Concurrent-refresh guard. Two clients carrying the same token
    // can both pass the lookup above (revoked_at is still NULL on the
    // shared row), then both call revoke_session, then both call
    // mint_session — yielding two diverged live sessions. Make the
    // revoke the synchronization point: only the caller whose UPDATE
    // actually flipped revoked_at gets to mint. The loser sees
    // SESSION_EXPIRED and re-logs-in (or, if a real client, retries
    // the request and receives the winner's new token via whatever
    // higher-level coordination it has).
    let did_revoke = state.repo.revoke_session(&session.id, now_ms).await?;
    if !did_revoke {
        return Err(AppError::session_expired(
            "session was concurrently refreshed by another caller",
        ));
    }

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
    // Idempotent: we're fine if a concurrent logout/refresh already
    // flipped revoked_at — the user's intent ("I want this token dead")
    // is satisfied either way.
    let _ = state.repo.revoke_session(&session.id, now_ms).await?;
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

    /// A session whose `expires_at` is in the past must come back as
    /// `SESSION_EXPIRED` from `/v1/auth/refresh`. The lookup query
    /// already filters `expires_at > ?`, so this is a regression
    /// guard — collapsing the filter would silently let expired
    /// sessions refresh themselves indefinitely.
    #[tokio::test(flavor = "current_thread")]
    async fn refresh_with_expired_session_returns_session_expired() {
        let fx = build_test_router().await;

        // Insert a session whose expires_at is way in the past, then
        // forge an Authorization header for its token by hashing the
        // raw token the way mint_session would.
        let now = now_ms();
        let user_id = fx
            .state
            .repo
            .upsert_user_by_email("expired@example.com", now)
            .await
            .unwrap();
        let raw_token = generate_session_token();
        let token_hash = sha256_b64url(raw_token.as_bytes());
        let _ = fx
            .state
            .repo
            .insert_auth_session(&user_id, &token_hash, now - 1_000_000, now - 100)
            .await
            .unwrap();

        let resp = fx
            .router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/refresh")
                    .header("authorization", format!("Bearer {raw_token}"))
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

    /// Two `/v1/auth/request` calls in quick succession against the
    /// same email must rate-limit the second. Without this guard a
    /// 600/min global budget could be spent entirely against one
    /// victim's inbox once Resend lands. Also checks that the standard
    /// `Retry-After` HTTP header is set (RFC 7231 §7.1.3) — proxies and
    /// off-the-shelf retry libraries read the header, not the JSON.
    #[tokio::test(flavor = "current_thread")]
    async fn request_within_cooldown_returns_rate_limited() {
        let fx = build_test_router().await;

        let body = json!({ "email": "cooldown@example.com" });

        // First request succeeds.
        let resp = fx
            .router
            .clone()
            .oneshot(json_request("POST", "/v1/auth/request", body.clone()).await)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Immediate second request must be rate-limited with a usable
        // retry_after_secs in BOTH the body and the `Retry-After` header.
        let resp = fx
            .router
            .oneshot(json_request("POST", "/v1/auth/request", body).await)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let header_retry = resp
            .headers()
            .get("retry-after")
            .expect("Retry-After header must be present on 429")
            .to_str()
            .unwrap()
            .parse::<u64>()
            .expect("Retry-After must be a number of seconds");
        assert!(
            (1..=60).contains(&header_retry),
            "Retry-After header out of range: {header_retry}"
        );

        let (_, body) = read_json(resp).await;
        assert_eq!(body["error"]["code"], "RATE_LIMITED");
        let body_retry = body["error"]["retry_after_secs"].as_u64().unwrap();
        assert_eq!(
            body_retry, header_retry,
            "body and header must agree on retry-after"
        );
    }

    /// A code that has been locked by 5 wrong guesses is functionally
    /// dead, so the per-email cooldown must not treat it as a "live"
    /// row — otherwise the user has to wait 60 s after lockout before
    /// requesting a fresh code, which is confusing UX. The cooldown
    /// SQL filters on `attempts < MAX_CODE_ATTEMPTS`; this test
    /// verifies the route honours that.
    #[tokio::test(flavor = "current_thread")]
    async fn locked_code_does_not_block_immediate_re_request() {
        let fx = build_test_router().await;
        let email = "lock@example.com";
        let request_id = seed_code(&fx.state, email, "111111").await;

        // Exhaust the attempt cap with wrong codes.
        for _ in 0..crate::turso::MAX_CODE_ATTEMPTS {
            fx.router
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
        }

        // The locked row's `consumed_at` is still NULL and `expires_at`
        // is still in the future — but the cooldown query should skip
        // it because `attempts >= MAX`. A fresh /v1/auth/request must
        // succeed immediately rather than 429.
        let resp = fx
            .router
            .oneshot(json_request("POST", "/v1/auth/request", json!({ "email": email })).await)
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "locked code blocked the re-request — cooldown SQL is missing the attempts filter"
        );
    }

    /// `revoke_session` must be the synchronization point for
    /// concurrent refresh: the first call wins, every subsequent call
    /// for the same `session_id` returns false. Direct repo test (a
    /// router-level test would need real concurrency to trigger).
    #[tokio::test(flavor = "current_thread")]
    async fn revoke_session_returns_true_only_for_the_first_caller() {
        let fx = build_test_router().await;
        let now = now_ms();
        let user_id = fx
            .state
            .repo
            .upsert_user_by_email("race@example.com", now)
            .await
            .unwrap();
        let session_id = fx
            .state
            .repo
            .insert_auth_session(&user_id, "fake-hash", now, now + 1_000_000)
            .await
            .unwrap();

        assert!(fx
            .state
            .repo
            .revoke_session(&session_id, now)
            .await
            .unwrap());
        assert!(!fx
            .state
            .repo
            .revoke_session(&session_id, now + 1)
            .await
            .unwrap());
    }

    /// Exercises the production transactional path
    /// (`try_insert_magic_code_with_cooldown`) end-to-end on the repo.
    /// Seeds an expired row, then calls the production method with the
    /// cooldown disabled, and verifies (a) the new row is in place and
    /// (b) the stale row was reaped — proving the reaper runs inside
    /// the transaction the route relies on, not just in the test-only
    /// `insert_magic_code` primitive.
    #[tokio::test(flavor = "current_thread")]
    async fn try_insert_with_cooldown_reaps_inside_transaction() {
        let fx = build_test_router().await;
        let email = "txn-reap@example.com";

        // Stale row: expired.
        let stale_id = uuid::Uuid::now_v7().to_string();
        fx.state
            .repo
            .insert_magic_code(
                &stale_id,
                email,
                &hash_code(&stale_id, "111111"),
                0,
                1, // expires_at way in the past
            )
            .await
            .unwrap();

        // Production path with cooldown=0 so the cooldown gate is a
        // no-op; we only care about the reap+insert atomic block.
        let now = now_ms();
        let fresh_id = uuid::Uuid::now_v7().to_string();
        let outcome = fx
            .state
            .repo
            .try_insert_magic_code_with_cooldown(
                &fresh_id,
                email,
                &hash_code(&fresh_id, "222222"),
                now,
                now + CODE_TTL_MS,
                0,
            )
            .await
            .unwrap();
        assert_eq!(outcome, crate::turso::InsertMagicCodeOutcome::Inserted);

        // Stale row must be gone (consume returns InvalidCode); fresh
        // row is consumable.
        let resp = fx
            .router
            .clone()
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": stale_id, "code": "111111" }),
                )
                .await,
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = fx
            .router
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": fresh_id, "code": "222222" }),
                )
                .await,
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// `insert_magic_code` opportunistically reaps expired or consumed
    /// rows for the same email. Without this the table grows without
    /// bound for any active address.
    #[tokio::test(flavor = "current_thread")]
    async fn insert_magic_code_reaps_dead_rows_for_same_email() {
        let fx = build_test_router().await;
        let email = "reap@example.com";

        // An expired row.
        let stale_request_id = uuid::Uuid::now_v7().to_string();
        fx.state
            .repo
            .insert_magic_code(
                &stale_request_id,
                email,
                &hash_code(&stale_request_id, "111111"),
                0,
                1, // already past
            )
            .await
            .unwrap();

        // A second insert for the same email at "now" should reap the
        // stale row before inserting the new one.
        let fresh_request_id = uuid::Uuid::now_v7().to_string();
        let now = now_ms();
        fx.state
            .repo
            .insert_magic_code(
                &fresh_request_id,
                email,
                &hash_code(&fresh_request_id, "222222"),
                now,
                now + CODE_TTL_MS,
            )
            .await
            .unwrap();

        // Verify the stale row no longer matches by trying to consume
        // it — this also exercises the route layer's collapse-into-
        // INVALID_CODE behaviour.
        let resp = fx
            .router
            .oneshot(
                json_request(
                    "POST",
                    "/v1/auth/verify",
                    json!({ "request_id": stale_request_id, "code": "111111" }),
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
    fn normalize_email_rejects_over_254_chars() {
        // RFC 5321 caps total email length at 254 octets — anything
        // longer is automatically invalid.
        let local = "a".repeat(243);
        let too_long = format!("{local}@example.com"); // 243 + 1 + 11 = 255
        assert_eq!(too_long.len(), 255);
        assert!(normalize_email(&too_long).is_err());

        let exactly_254 = format!("{}@example.com", "a".repeat(242));
        assert_eq!(exactly_254.len(), 254);
        assert!(normalize_email(&exactly_254).is_ok());
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
