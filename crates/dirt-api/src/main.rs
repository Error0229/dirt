//! Local development server entry point.
//!
//! Reads `.env.server` when built in debug mode, constructs the same
//! `AppState` / `Router` the Vercel adapter uses, and serves on the
//! configured bind address.
//!
//! Note: we build the tokio runtime manually on an 8 MiB stack instead of
//! using `#[tokio::main]`. libsql + axum startup on Windows routinely
//! overflows the default 1 MiB main-thread stack (see the same workaround
//! in `dirt-core::services::database`).

use std::sync::Arc;

use dirt_api::{build_router, AppConfig, AppState, TursoRepo};

const STACK_SIZE: usize = 8 * 1024 * 1024;

#[cfg(debug_assertions)]
fn load_dev_dotenv() {
    let server_env = std::path::Path::new(".env.server");
    if server_env.exists() {
        let _ = dotenvy::from_path(server_env);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(debug_assertions)]
    load_dev_dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt_api=info".parse()?),
        )
        .init();

    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .name("dirt-api-runtime".into())
        .spawn(run_server)?
        .join()
        .map_err(|_| "server thread panicked")?
}

fn run_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(STACK_SIZE)
        .build()?;

    runtime.block_on(async {
        let config = Arc::new(AppConfig::from_env()?);
        tracing::info!("Starting dirt-api with config: {:?}", config);

        let repo = Arc::new(
            TursoRepo::connect(&config.turso_database_url, &config.turso_auth_token).await?,
        );
        let state = AppState::new(config.clone(), repo);

        let bind_addr = config.bind_addr.clone();
        let router = build_router(state);

        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        tracing::info!("dirt-api listening on {}", bind_addr);
        axum::serve(listener, router).await?;
        Ok(())
    })
}
