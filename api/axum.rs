//! Vercel serverless entry point.
//!
//! Reads the same environment variables as `dirt-api serve` (set on the
//! Vercel project), builds the same router, and wraps it with
//! `vercel_runtime::axum::VercelLayer` so Vercel can drive it as a function.
//!
//! One function holds the whole router; `vercel.json` rewrites every path to
//! it. Vercel's Root Directory for the project is the workspace root.
//!
//! The pool and the signing keys are built here, before `run`, so that warm
//! invocations of one instance share them. Neither connects until first use.

use dirt_api::{build_router, AppState, Config};
use tower::ServiceBuilder;
use vercel_runtime::axum::VercelLayer;
use vercel_runtime::Error;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let config =
        Config::from_env().map_err(|e| Error::from(format!("failed to load config: {e}")))?;
    let state = AppState::from_config(config)
        .map_err(|e| Error::from(format!("failed to build the server state: {e}")))?;

    let app = ServiceBuilder::new()
        .layer(VercelLayer::new())
        .service(build_router(state));

    vercel_runtime::run(app).await
}
