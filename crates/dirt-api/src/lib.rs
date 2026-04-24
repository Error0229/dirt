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

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub use config::AppConfig;
pub use error::AppError;
pub use turso::TursoRepo;

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
    let authed = Router::new()
        .route("/v1/notes/push", post(routes::push_notes))
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
