//! `dirt-api`: run the server locally, or apply the database schema.
//!
//! ```text
//! dirt-api            # serve on DIRT_API_BIND_ADDR (default 0.0.0.0:8080)
//! dirt-api migrate    # apply migrations to MIGRATE_DATABASE_URL
//! ```
//!
//! Both read `.env.server` from the working directory if it is there.
//!
//! Migrations are a separate command on purpose. They run once at deploy
//! time against the database's direct endpoint: the migration lock is held
//! for a whole session, which a pooled endpoint does not provide. The server
//! itself never changes the schema.

use std::error::Error;

use dirt_api::{build_router, AppState, Config, MIGRATOR};
use sqlx::postgres::PgPoolOptions;

type AnyError = Box<dyn Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let _ = dotenvy::from_filename(".env.server");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dirt_api=info,tower_http=info".into()),
        )
        .init();

    match std::env::args().nth(1).as_deref() {
        None | Some("serve") => serve().await,
        Some("migrate") => migrate().await,
        Some(other) => Err(format!("unknown command {other:?}; use `serve` or `migrate`").into()),
    }
}

async fn serve() -> Result<(), AnyError> {
    let config = Config::from_env()?;
    let address = config.bind_addr;
    let app = build_router(AppState::from_config(config)?);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("dirt-api listening on {address}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn migrate() -> Result<(), AnyError> {
    let url = std::env::var("MIGRATE_DATABASE_URL")
        .map_err(|_| "MIGRATE_DATABASE_URL is not set; use the database's direct endpoint")?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    MIGRATOR.run(&pool).await?;
    for migration in MIGRATOR.iter() {
        tracing::info!("applied: {} {}", migration.version, migration.description);
    }
    Ok(())
}
