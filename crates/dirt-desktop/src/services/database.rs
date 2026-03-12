//! Desktop wrapper for the shared core database service.

#![allow(dead_code)] // Methods are consumed through Deref from app components.

use std::ops::Deref;
use std::path::PathBuf;
use std::thread;

use dirt_core::db::SyncConfig;
use dirt_core::models::Settings;
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

    /// Run a sync cycle on a dedicated large-stack thread.
    ///
    /// libSQL sync can exceed the default Windows main-thread stack depth.
    pub async fn sync_with_large_stack(&self) -> Result<()> {
        let service = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| dirt_core::Error::Database(error.to_string()))?
                        .block_on(service.sync())
                })
                .map_err(|error| dirt_core::Error::Database(error.to_string()))?
                .join()
                .map_err(|_| dirt_core::Error::Database("Sync thread panicked".to_string()))?
        })
        .await
        .map_err(|error| dirt_core::Error::Database(error.to_string()))?
    }

    /// Load settings on a dedicated large-stack thread.
    pub async fn load_settings_with_large_stack(&self) -> Result<Settings> {
        let service = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| dirt_core::Error::Database(error.to_string()))?
                        .block_on(service.load_settings())
                })
                .map_err(|error| dirt_core::Error::Database(error.to_string()))?
                .join()
                .map_err(|_| dirt_core::Error::Database("Settings thread panicked".to_string()))?
        })
        .await
        .map_err(|error| dirt_core::Error::Database(error.to_string()))?
    }

    fn default_db_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| panic!("Failed to resolve desktop data directory"))
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
