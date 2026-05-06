//! Session-token authentication middleware.
//!
//! Phase 2.2 replaces the shared `DIRT_SERVER_TOKEN` bearer with
//! per-user session tokens minted by `/v1/auth/verify`. The middleware
//! peels the bearer off the request, sha256-hashes it, looks up the
//! `auth_sessions` row, and attaches the resolved `user_id` to the
//! request as an `Extension<UserId>`. Notes handlers downstream pull
//! that extension instead of reading a hard-coded `SOLO_USER_ID`.
//!
//! Every "missing / malformed / unknown / revoked / expired token"
//! condition collapses to `SESSION_EXPIRED` (401) on the wire — the
//! client's response is always the same ("show the login screen"), and
//! distinguishing the cases would let an attacker probe for live
//! `token_hash` values via differential error messages.
//!
//! The hash helpers live here (rather than in `routes_auth`) because
//! both modules need them and the auth module is the right home for
//! anything that handles session tokens.

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap};
use axum::middleware::Next;
use axum::response::Response;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::AppState;

pub(crate) const BEARER_PREFIX_LEN: usize = "Bearer ".len();

/// Resolved user id for an authenticated request.
///
/// Attached as an extension by `require_session` so the notes handlers
/// can pull it out without re-doing the session lookup. Wrapped in a
/// newtype so a stray `Extension<String>` somewhere else in the stack
/// can't accidentally collide with it.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
}

/// Middleware applied to `/v1/notes/*`. Verifies the session token,
/// throttle-bumps `last_used_at`, and inserts an `AuthenticatedUser`
/// extension for the handler.
pub async fn require_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = extract_bearer(request.headers())?;
    let token_hash = sha256_b64url(token.as_bytes());
    let now_ms = chrono::Utc::now().timestamp_millis();

    let session = state
        .repo
        .lookup_session_by_token_hash(&token_hash, now_ms)
        .await?
        .ok_or_else(|| AppError::session_expired("session token is invalid or expired"))?;

    request.extensions_mut().insert(AuthenticatedUser {
        user_id: session.user_id,
    });
    Ok(next.run(request).await)
}

/// Pull the bearer token off the `Authorization` header. Every error
/// path returns `SESSION_EXPIRED` so client responses are uniform.
pub(crate) fn extract_bearer(headers: &HeaderMap) -> Result<String, AppError> {
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

/// sha256(input), base64url-no-pad. The on-disk encoding for every
/// secret hash in the auth schema (`magic_codes.code_hash`,
/// `auth_sessions.token_hash`).
pub(crate) fn sha256_b64url(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::TursoRepo;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::{middleware, Extension, Router};
    use std::sync::Arc;
    use tower::ServiceExt;

    /// End-to-end smoke test: a real session row in the DB, a real
    /// bearer header, the middleware resolves it and the inner handler
    /// sees the `AuthenticatedUser` extension.
    #[tokio::test(flavor = "current_thread")]
    async fn valid_session_token_attaches_user_id() {
        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let user_id = temp_db
            .repo
            .upsert_user_by_email("auth@example.com", now)
            .await
            .unwrap();
        let raw_token = "test-bearer-token-with-enough-bytes";
        let token_hash = sha256_b64url(raw_token.as_bytes());
        temp_db
            .repo
            .insert_auth_session(&user_id, &token_hash, now, now + 1_000_000)
            .await
            .unwrap();

        let state = test_state(Arc::clone(&temp_db.repo));
        let router = Router::new()
            .route(
                "/probe",
                get(|Extension(user): Extension<AuthenticatedUser>| async move { user.user_id }),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                require_session,
            ))
            .with_state(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/probe")
                    .header("authorization", format!("Bearer {raw_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), user_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_header_returns_session_expired() {
        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let state = test_state(Arc::clone(&temp_db.repo));
        let router = build_probe(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unknown_token_returns_session_expired() {
        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let state = test_state(Arc::clone(&temp_db.repo));
        let router = build_probe(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/probe")
                    .header("authorization", "Bearer never-minted-this-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn revoked_session_returns_session_expired() {
        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let user_id = temp_db
            .repo
            .upsert_user_by_email("revoked@example.com", now)
            .await
            .unwrap();
        let raw_token = "soon-to-be-revoked-bearer-token";
        let token_hash = sha256_b64url(raw_token.as_bytes());
        let session_id = temp_db
            .repo
            .insert_auth_session(&user_id, &token_hash, now, now + 1_000_000)
            .await
            .unwrap();
        let revoked = temp_db.repo.revoke_session(&session_id, now).await.unwrap();
        assert!(revoked, "first revoke should flip the row");

        let state = test_state(Arc::clone(&temp_db.repo));
        let router = build_probe(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/probe")
                    .header("authorization", format!("Bearer {raw_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    fn build_probe(state: AppState) -> Router {
        Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                require_session,
            ))
            .with_state(state)
    }

    fn test_state(repo: Arc<TursoRepo>) -> AppState {
        let config = AppConfig {
            bind_addr: "127.0.0.1:0".into(),
            turso_database_url: "libsql://unused.test".into(),
            turso_auth_token: "unused".into(),
        };
        AppState {
            config: Arc::new(config),
            repo,
            email: Arc::new(crate::email::EmailSender::log_only()),
        }
    }
}
