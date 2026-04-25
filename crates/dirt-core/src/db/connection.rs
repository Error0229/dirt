//! Database connection management.
//!
//! Local-only since the Supabase removal: every client now syncs through
//! the `dirt-api` HTTP backend (see `dirt_core::sync::api_client`) instead
//! of via Turso's embedded-replica builder.

use crate::error::Result;
use libsql::{Builder, Connection, Database as LibSqlDatabase};
use std::path::Path;

use super::migrations;

/// Database wrapper for libSQL connections.
pub struct Database {
    #[allow(dead_code)]
    db: LibSqlDatabase,
    conn: Connection,
}

impl Database {
    /// Open a local-only database at the given path, creating it if it
    /// doesn't exist. Runs migrations automatically.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let db = Builder::new_local(&path_str).build().await?;
        let conn = db.connect()?;

        let database = Self { db, conn };
        database.configure().await?;
        database.migrate().await?;
        Ok(database)
    }

    /// Open an in-memory database (useful for testing).
    pub async fn open_in_memory() -> Result<Self> {
        let db = Builder::new_local(":memory:").build().await?;
        let conn = db.connect()?;

        let database = Self { db, conn };
        database.configure().await?;
        database.migrate().await?;
        Ok(database)
    }

    /// Configure `SQLite` for optimal performance.
    async fn configure(&self) -> Result<()> {
        self.conn
            .execute("PRAGMA journal_mode = WAL;", ())
            .await
            .ok();
        self.conn
            .execute("PRAGMA synchronous = NORMAL;", ())
            .await
            .ok();
        self.conn.execute("PRAGMA foreign_keys = ON;", ()).await?;
        self.conn
            .execute("PRAGMA cache_size = 10000;", ())
            .await
            .ok();
        Ok(())
    }

    /// Run database migrations.
    async fn migrate(&self) -> Result<()> {
        migrations::run(&self.conn).await
    }

    /// Get a reference to the underlying connection.
    pub const fn connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_open_in_memory() {
        let _ = Database::open_in_memory().await.unwrap();
    }
}
