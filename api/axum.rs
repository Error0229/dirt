//! Vercel serverless entry point.
//!
//! Reads environment variables from the Vercel project (set via
//! `vercel env add` or the dashboard), builds the same `AppState` /
//! `Router` the local dev binary uses, and wraps it with
//! `vercel_runtime::axum::VercelLayer` so Vercel can drive it as a
//! Function.
//!
//! Vercel exposes each `api/*.rs` file at its own route prefix, but we
//! use a single `api/axum.rs` so the Router we already build in
//! `dirt_api::build_router` handles all routing internally. Deployed by
//! setting Vercel's Root Directory to `crates/dirt-api/`.

use std::sync::Arc;

use dirt_api::{build_router, AppConfig, AppState, TursoRepo};
use tower::ServiceBuilder;
use vercel_runtime::axum::VercelLayer;
use vercel_runtime::Error;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let config = Arc::new(
        AppConfig::from_env().map_err(|e| Error::from(format!("failed to load config: {e}")))?,
    );
    let repo = Arc::new(
        TursoRepo::connect(&config.turso_database_url, &config.turso_auth_token)
            .await
            .map_err(|e| Error::from(format!("failed to connect to Turso: {e}")))?,
    );
    let state = AppState::new(config, repo);
    let router = build_router(state);

    let app = ServiceBuilder::new()
        .layer(VercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}
