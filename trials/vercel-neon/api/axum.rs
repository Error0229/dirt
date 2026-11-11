//! The Vercel function. Everything interesting is in `src/lib.rs`.
//!
//! Environment, set on the Vercel project:
//! - `DATABASE_URL`: Neon's pooled connection string.
//! - `TRIAL_TOKEN`: shared secret the load script sends.
//! - `STATEMENT_CACHE` (optional): sqlx's prepared-statement cache size; 0
//!   turns the cache off.

use tower::ServiceBuilder;
use vercel_neon_trial::{build_router, lazy_pool, AppState};
use vercel_runtime::axum::VercelLayer;
use vercel_runtime::Error;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let url = std::env::var("DATABASE_URL").map_err(|_| Error::from("DATABASE_URL is not set"))?;
    let token = std::env::var("TRIAL_TOKEN").map_err(|_| Error::from("TRIAL_TOKEN is not set"))?;
    let statement_cache = std::env::var("STATEMENT_CACHE")
        .ok()
        .and_then(|text| text.parse().ok());

    // Built before `run`, so warm invocations of this instance share it. It
    // does not connect until the first query.
    let pool = lazy_pool(&url, statement_cache)
        .map_err(|error| Error::from(format!("bad DATABASE_URL: {error}")))?;

    let app = ServiceBuilder::new()
        .layer(VercelLayer::new())
        .service(build_router(AppState { pool, token }));

    vercel_runtime::run(app).await
}
