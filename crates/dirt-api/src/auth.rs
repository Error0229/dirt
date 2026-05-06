//! Bearer-token authentication middleware.
//!
//! Verifies `Authorization: Bearer <token>` against `DIRT_SERVER_TOKEN`
//! using `subtle::ConstantTimeEq` to avoid leaking the token through
//! response-timing differences. Solo-phase authentication: every valid
//! token resolves to `SOLO_USER_ID`; the handler layer reads that
//! constant directly, so there's no per-request `user_id` threading yet.

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use crate::error::AppError;
use crate::AppState;

const BEARER_PREFIX_LEN: usize = "Bearer ".len();

pub async fn require_bearer_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let header_value = request
        .headers()
        .get(header::AUTHORIZATION)
        .ok_or_else(|| AppError::unauthorized("missing Authorization header"))?;

    let header_str = header_value
        .to_str()
        .map_err(|_| AppError::unauthorized("Authorization header is not valid UTF-8"))?;

    // RFC 7235 §2.1: auth-scheme names are case-insensitive. Match the
    // prefix on a lowercased copy, then slice the original at the same
    // offset so the token's own case is preserved for ConstantTimeEq.
    if header_str.len() < BEARER_PREFIX_LEN
        || !header_str[..BEARER_PREFIX_LEN].eq_ignore_ascii_case("Bearer ")
    {
        return Err(AppError::unauthorized(
            "Authorization header must start with 'Bearer '",
        ));
    }
    let token = &header_str[BEARER_PREFIX_LEN..];

    let token_bytes = token.as_bytes();
    let expected = state.config.server_token.0.as_slice();

    // ConstantTimeEq only compares equal-length slices in constant time.
    // Different lengths obviously differ and we short-circuit with an Err —
    // the length check itself isn't a timing attack surface because the
    // attacker already controls the length they send.
    if token_bytes.len() != expected.len() {
        return Err(AppError::unauthorized("invalid bearer token"));
    }

    if bool::from(token_bytes.ct_eq(expected)) {
        Ok(next.run(request).await)
    } else {
        Err(AppError::unauthorized("invalid bearer token"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ServerToken};
    use crate::TursoRepo;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::{middleware, Router};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_state(token: &str) -> AppState {
        let config = AppConfig {
            bind_addr: "127.0.0.1:0".into(),
            turso_database_url: "libsql://unused.test".into(),
            turso_auth_token: "unused".into(),
            server_token: ServerToken(token.as_bytes().to_vec()),
        };
        AppState {
            config: Arc::new(config),
            // Repo and email are never touched by the middleware itself.
            repo: Arc::new(TursoRepo::dangling()),
            email: Arc::new(crate::email::EmailSender::log_only()),
        }
    }

    fn build_test_router(token: &str) -> Router {
        let state = test_state(token);
        Router::new()
            .route("/guarded", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                require_bearer_token,
            ))
            .with_state(state)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_missing_header() {
        let router = build_test_router("abcdef1234567890");
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/guarded")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_wrong_token() {
        let router = build_test_router("abcdef1234567890");
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/guarded")
                    .header("authorization", "Bearer wrong-token-value")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepts_correct_token() {
        let token = "abcdef1234567890";
        let router = build_test_router(token);
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/guarded")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepts_lowercase_scheme() {
        let token = "abcdef1234567890";
        let router = build_test_router(token);
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/guarded")
                    .header("authorization", format!("bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepts_uppercase_scheme() {
        let token = "abcdef1234567890";
        let router = build_test_router(token);
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/guarded")
                    .header("authorization", format!("BEARER {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_short_header() {
        let router = build_test_router("abcdef1234567890");
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/guarded")
                    .header("authorization", "Bear")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_mismatched_length() {
        let router = build_test_router("abcdef1234567890");
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/guarded")
                    .header("authorization", "Bearer short")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
