//! Runs the same router on localhost, without Vercel, for trying the load
//! script before deploying. Reads the same environment as the function.

use vercel_neon_trial::{build_router, lazy_pool, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is not set")?;
    let token = std::env::var("TRIAL_TOKEN").map_err(|_| "TRIAL_TOKEN is not set")?;
    let statement_cache = std::env::var("STATEMENT_CACHE")
        .ok()
        .and_then(|text| text.parse().ok());
    let pool = lazy_pool(&url, statement_cache)?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787").await?;
    println!("listening on http://127.0.0.1:8787");
    axum::serve(listener, build_router(AppState { pool, token })).await?;
    Ok(())
}
