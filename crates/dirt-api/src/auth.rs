//! Bearer-token authentication middleware.
//!
//! Verifies `Authorization: Bearer <token>` against `DIRT_SERVER_TOKEN`
//! using `subtle::ConstantTimeEq` to avoid leaking the token through
//! response-timing differences. Solo-phase authentication: every valid
//! token resolves to `SOLO_USER_ID`; the handler layer reads that
//! constant directly, so there's no per-request user_id threading yet.

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use crate::AppState;
use crate::error::AppError;

const BEARER_PREFIX: &str = "Bearer ";

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

    let Some(token) = header_str.strip_prefix(BEARER_PREFIX) else {
        return Err(AppError::unauthorized(
            "Authorization header must start with 'Bearer '",
        ));
    };

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
    use crate::TursoRepo;
    use crate::config::{AppConfig, ServerToken};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::{Router, middleware};
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
            // Repo is never touched by the middleware itself.
            repo: Arc::new(TursoRepo::dangling()),
        }
    }

    async fn build_test_router(token: &str) -> Router {
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
        let router = build_test_router("abcdef1234567890").await;
        let resp = router
            .oneshot(Request::builder().uri("/guarded").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_wrong_token() {
        let router = build_test_router("abcdef1234567890").await;
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
        let router = build_test_router(token).await;
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
    async fn rejects_mismatched_length() {
        let router = build_test_router("abcdef1234567890").await;
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
