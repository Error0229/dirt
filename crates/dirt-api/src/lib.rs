//! Dirt sync backend.
//!
//! Exposes a tiny axum router with three routes:
//!   - `GET  /healthz`         — liveness probe, no auth.
//!   - `POST /v1/notes/push`   — client pushes a batch of notes.
//!   - `GET  /v1/notes/pull`   — client pulls notes changed after a cursor.
//!
//! Authentication is a single shared bearer token verified in
//! constant time against `DIRT_SERVER_TOKEN`. Solo-phase only; Phase 2
//! replaces this with per-user session tokens.

pub mod auth;
pub mod config;
pub mod error;
pub mod routes;
pub mod turso;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub use config::AppConfig;
pub use error::AppError;
pub use turso::TursoRepo;

/// Maximum acceptable request body size for `/v1/notes/push`.
///
/// Sized so a 500-note batch of ~10 KiB notes (5 MiB) fits with
/// headroom while preventing accidental OOM or abuse from oversized
/// payloads. Bodies larger than this short-circuit with HTTP 413
/// before reaching the handler.
pub const PUSH_BODY_LIMIT: usize = 8 * 1024 * 1024;

/// Shared state threaded through every handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub repo: Arc<TursoRepo>,
}

impl AppState {
    #[must_use]
    pub const fn new(config: Arc<AppConfig>, repo: Arc<TursoRepo>) -> Self {
        Self { config, repo }
    }
}

/// Build the axum router with auth middleware applied to the `/v1/*`
/// routes. Consumed by both the local dev binary and the Vercel adapter.
pub fn build_router(state: AppState) -> Router {
    // Body limit is layered on the push route specifically; pull is a
    // GET so the limit is moot, and applying it globally would also cap
    // the (currently empty) `/healthz` response which would just be
    // noise.
    let push = post(routes::push_notes).layer(DefaultBodyLimit::max(PUSH_BODY_LIMIT));

    let authed = Router::new()
        .route("/v1/notes/push", push)
        .route("/v1/notes/pull", get(routes::pull_notes))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer_token,
        ));

    Router::new()
        .route("/healthz", get(routes::healthz))
        .merge(authed)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use config::ServerToken;
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
            repo: Arc::new(TursoRepo::dangling()),
        }
    }

    /// Body-limit layer rejects oversized push payloads before the
    /// auth middleware or handler can run; a missing token yields the
    /// usual 401 only when the body fits.
    #[tokio::test(flavor = "current_thread")]
    async fn push_rejects_oversized_body_with_413() {
        let router = build_router(test_state("abcdef1234567890abcd"));

        // 9 MiB of zeros — comfortably above PUSH_BODY_LIMIT (8 MiB).
        let oversized = vec![b'x'; PUSH_BODY_LIMIT + 1024 * 1024];
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/notes/push")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer abcdef1234567890abcd")
                    .body(Body::from(oversized))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
