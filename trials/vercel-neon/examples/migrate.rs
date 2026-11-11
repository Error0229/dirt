//! Applies `migrations/` to the database named by `MIGRATE_DATABASE_URL`.
//!
//! Use Neon's direct endpoint. sqlx takes a session-level advisory lock while
//! it migrates, and the pooled endpoint hands each transaction to a different
//! server connection, so the lock and its release can land on different
//! sessions.

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url =
        std::env::var("MIGRATE_DATABASE_URL").map_err(|_| "MIGRATE_DATABASE_URL is not set")?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let version: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&pool)
        .await?;
    let applied: Vec<(i64, String)> =
        sqlx::query_as("SELECT version, description FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await?;
    println!("server version: {version}");
    for (number, description) in applied {
        println!("applied: {number} {description}");
    }
    Ok(())
}
