//! Desktop wrapper for the shared core database service.

#![allow(dead_code)] // Methods are consumed through Deref from app components.

use std::ops::Deref;
use std::path::PathBuf;

use dirt_core::db::SyncConfig;
use dirt_core::services::DatabaseService as CoreDatabaseService;
use dirt_core::Result;

/// Desktop database service preserving desktop path defaults.
#[derive(Clone)]
pub struct DatabaseService {
    inner: CoreDatabaseService,
}

impl DatabaseService {
    /// Create a new local-only database service.
    pub async fn new() -> Result<Self> {
        let inner = CoreDatabaseService::open_local_path(Self::default_db_path()).await?;
        Ok(Self { inner })
    }

    /// Create a new sync-enabled database service.
    pub async fn new_with_sync(sync_config: SyncConfig) -> Result<Self> {
        let inner =
            CoreDatabaseService::open_sync_path(Self::default_db_path(), sync_config).await?;
        Ok(Self { inner })
    }

    /// Create an in-memory database service.
    pub async fn in_memory() -> Result<Self> {
        let inner = CoreDatabaseService::open_in_memory().await?;
        Ok(Self { inner })
    }

    fn default_db_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dirt")
            .join("dirt.db")
    }
}

impl Deref for DatabaseService {
    type Target = CoreDatabaseService;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
