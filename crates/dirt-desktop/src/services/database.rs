//! Desktop wrapper for the shared core database service.
//!
//! Path resolution sits in `dirt_core::services::db_paths` since
//! Phase 2.x — the desktop wrapper owns the large-stack settings
//! load and a thin `new_for_user` convenience constructor; everything
//! else delegates to [`dirt_core::services::DatabaseService`] via
//! `Deref`.

#![allow(dead_code)] // Methods are consumed through Deref from app components.

use std::ops::Deref;
use std::path::PathBuf;
use std::thread;

use dirt_core::models::Settings;
use dirt_core::services::db_paths::{solo_db_path as core_solo_db_path, user_db_path, DB_FILENAME};
use dirt_core::services::DatabaseService as CoreDatabaseService;
use dirt_core::Result;

/// Desktop database service preserving desktop path defaults.
#[derive(Clone)]
pub struct DatabaseService {
    inner: CoreDatabaseService,
}

impl DatabaseService {
    /// Open the per-user DB for `user_id` under the desktop's
    /// canonical `<os_data>/dirt` directory.
    ///
    /// Used by the login swap path in `components::settings::
    /// account_settings::apply_login_outcome` and by startup when
    /// `state.json` is present.
    pub async fn open_for_user(user_id: &str) -> Result<Self> {
        let path = user_db_path(&Self::data_dir(), user_id, DB_FILENAME)?;
        let inner = CoreDatabaseService::open_for_user(path, user_id).await?;
        Ok(Self { inner })
    }

    /// Open the legacy pre-signin solo DB.
    ///
    /// Only reachable on a brand-new desktop install that has never
    /// signed in. Once a sign-in lands, the migration moves this file
    /// into the user's directory and subsequent launches go through
    /// [`Self::open_for_user`].
    pub async fn open_solo() -> Result<Self> {
        let path = core_solo_db_path(&Self::data_dir(), DB_FILENAME);
        let inner = CoreDatabaseService::open_local_path(path).await?;
        Ok(Self { inner })
    }

    /// Create an in-memory database service.
    pub async fn in_memory() -> Result<Self> {
        let inner = CoreDatabaseService::open_in_memory().await?;
        Ok(Self { inner })
    }

    /// Load settings on a dedicated large-stack thread.
    ///
    /// libSQL operations can exceed the default Windows main-thread stack
    /// depth on Dioxus-hosted runtimes, hence the spawn-and-block dance.
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

    /// Canonical `<os_data>/dirt` directory shared with the CLI and
    /// mobile builds. The `DIRT_DATA_DIR` env var override is honored
    /// for tests + developer overrides.
    pub fn data_dir() -> PathBuf {
        if let Some(override_dir) = std::env::var_os("DIRT_DATA_DIR") {
            return PathBuf::from(override_dir);
        }
        dirs::data_dir()
            .unwrap_or_else(|| panic!("Failed to resolve desktop data directory"))
            .join("dirt")
    }
}

/// Borrow the desktop wrapper as the core service. Convenience for
/// call sites (sync worker, account settings) that already hold a
/// `&DatabaseService` and want to pass it to APIs typed against the
/// core wrapper.
impl AsRef<CoreDatabaseService> for DatabaseService {
    fn as_ref(&self) -> &CoreDatabaseService {
        &self.inner
    }
}

impl Deref for DatabaseService {
    type Target = CoreDatabaseService;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
