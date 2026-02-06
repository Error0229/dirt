//! Database connection management

use crate::error::Result;
use libsql::{Builder, Connection, Database as LibSqlDatabase};
use std::path::Path;
use std::time::Duration;

use super::migrations;

/// Configuration for database sync
#[derive(Debug, Clone, Default)]
pub struct SyncConfig {
    /// Remote database URL (e.g., `libsql://your-db.turso.io`)
    pub url: Option<String>,
    /// Authentication token for remote database
    pub auth_token: Option<String>,
    /// Automatic sync interval (default: 60 seconds)
    pub sync_interval: Option<Duration>,
}

impl SyncConfig {
    /// Create a new sync configuration
    pub fn new(url: impl Into<String>, auth_token: impl Into<String>) -> Self {
        Self {
            url: Some(url.into()),
            auth_token: Some(auth_token.into()),
            sync_interval: Some(Duration::from_secs(60)), // Default: sync every 60 seconds
        }
    }

    /// Set the automatic sync interval
    #[must_use]
    pub const fn with_sync_interval(mut self, interval: Duration) -> Self {
        self.sync_interval = Some(interval);
        self
    }

    /// Disable automatic sync (manual sync only)
    #[must_use]
    pub const fn without_auto_sync(mut self) -> Self {
        self.sync_interval = None;
        self
    }

    /// Check if sync is configured
    pub const fn is_configured(&self) -> bool {
        self.url.is_some() && self.auth_token.is_some()
    }
}

/// Database wrapper for libSQL connections
pub struct Database {
    db: LibSqlDatabase,
    conn: Connection,
    sync_config: Option<SyncConfig>,
}

impl Database {
    /// Open a local-only database at the given path, creating it if it doesn't exist
    ///
    /// Runs migrations automatically.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let db = Builder::new_local(&path_str).build().await?;
        let conn = db.connect()?;

        let database = Self {
            db,
            conn,
            sync_config: None,
        };
        database.configure().await?;
        database.migrate().await?;
        Ok(database)
    }

    /// Open an in-memory database (useful for testing)
    pub async fn open_in_memory() -> Result<Self> {
        let db = Builder::new_local(":memory:").build().await?;
        let conn = db.connect()?;

        let database = Self {
            db,
            conn,
            sync_config: None,
        };
        database.configure().await?;
        database.migrate().await?;
        Ok(database)
    }

    /// Open a database with embedded replica (syncs with remote Turso database)
    ///
    /// This creates a local `SQLite` file that syncs with a remote Turso database.
    /// Reads are served from the local file (fast), writes go to remote and sync back.
    pub async fn open_with_sync(
        local_path: impl AsRef<Path>,
        sync_config: SyncConfig,
    ) -> Result<Self> {
        let path_str = local_path.as_ref().to_string_lossy().to_string();

        let url = sync_config
            .url
            .as_ref()
            .ok_or_else(|| crate::error::Error::InvalidInput("Sync URL is required".into()))?;
        let token = sync_config
            .auth_token
            .as_ref()
            .ok_or_else(|| crate::error::Error::InvalidInput("Auth token is required".into()))?;

        let mut builder = Builder::new_remote_replica(&path_str, url.clone(), token.clone());

        // Configure automatic sync interval if specified (per Turso docs recommendation)
        if let Some(interval) = sync_config.sync_interval {
            builder = builder.sync_interval(interval);
            tracing::debug!("Automatic sync interval set to {:?}", interval);
        }

        let db = builder.build().await?;
        let conn = db.connect()?;

        let database = Self {
            db,
            conn,
            sync_config: Some(sync_config),
        };

        // Sync first to pull remote schema if it exists
        tracing::debug!("Performing initial sync...");
        database.sync().await?;

        // Then configure and migrate (migrations will create schema on remote if needed)
        database.configure().await?;
        database.migrate().await?;

        Ok(database)
    }

    /// Configure `SQLite` for optimal performance
    async fn configure(&self) -> Result<()> {
        // Enable WAL mode for better concurrency (local databases only)
        // Note: Some pragmas may not work with remote replicas
        self.conn
            .execute("PRAGMA journal_mode = WAL;", ())
            .await
            .ok(); // Ignore errors for remote replicas
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

    /// Run database migrations
    async fn migrate(&self) -> Result<()> {
        migrations::run(&self.conn).await
    }

    /// Sync with remote database (if configured)
    ///
    /// For embedded replicas, this pulls changes from the remote database.
    pub async fn sync(&self) -> Result<()> {
        if self.sync_config.is_some() {
            self.db.sync().await?;
            tracing::debug!("Database synced with remote");
        }
        Ok(())
    }

    /// Check if sync is configured
    pub const fn is_sync_enabled(&self) -> bool {
        self.sync_config.is_some()
    }

    /// Get a reference to the underlying connection
    pub const fn connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_open_in_memory() {
        let db = Database::open_in_memory().await.unwrap();
        assert!(!db.is_sync_enabled());
    }

    #[test]
    fn test_sync_config_new() {
        let config = SyncConfig::new("libsql://test.turso.io", "test-token");
        assert!(config.is_configured());
        assert_eq!(config.url, Some("libsql://test.turso.io".to_string()));
        assert_eq!(config.auth_token, Some("test-token".to_string()));
    }

    #[test]
    fn test_sync_config_default_not_configured() {
        let config = SyncConfig::default();
        assert!(!config.is_configured());
    }

    /// Integration test for Turso sync - only runs if env vars are set
    /// Run with: TURSO_DATABASE_URL=... TURSO_AUTH_TOKEN=... cargo test test_sync_with_turso -- --ignored
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "Requires TURSO_DATABASE_URL and TURSO_AUTH_TOKEN"]
    async fn test_sync_with_turso() {
        let url = env::var("TURSO_DATABASE_URL").expect("TURSO_DATABASE_URL must be set");
        let token = env::var("TURSO_AUTH_TOKEN").expect("TURSO_AUTH_TOKEN must be set");

        let config = SyncConfig::new(url, token);
        assert!(config.is_configured());

        // Use a temp directory for the local replica
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("test_sync.db");

        // Open with sync - this should connect to Turso and sync
        let db = Database::open_with_sync(&db_path, config).await.unwrap();
        assert!(db.is_sync_enabled());

        // Verify we can execute queries
        let mut rows = db
            .connection()
            .query("SELECT 1", ())
            .await
            .expect("Should be able to execute query");
        let row = rows.next().await.unwrap().unwrap();
        let val: i32 = row.get(0).unwrap();
        assert_eq!(val, 1);

        // Sync should work
        db.sync().await.expect("Sync should succeed");
    }
}
