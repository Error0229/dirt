mod auth;
mod config;
mod error;
mod media;
mod rate_limit;
mod routes;
mod turso;

use std::sync::Arc;

use config::AppConfig;
use routes::{app_router, AppState};

/// Load .env.server for local development.
#[cfg(debug_assertions)]
fn load_dev_dotenv() {
    let server_env = std::path::Path::new(".env.server");
    if server_env.exists() {
        let _ = dotenvy::from_path(server_env);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(debug_assertions)]
    load_dev_dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt_api=info".parse().expect("valid directive")),
        )
        .init();

    let config = Arc::new(AppConfig::from_env()?);
    tracing::info!("Starting dirt-api with config: {:?}", config);

    let state = AppState::from_config(config);
    let bind_addr = state.config.bind_addr.clone();
    let router = app_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("dirt-api listening on {}", bind_addr);
    axum::serve(listener, router).await?;
    Ok(())
}
