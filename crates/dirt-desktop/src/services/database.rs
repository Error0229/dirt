//! Where the desktop app keeps its note stores.

use std::path::PathBuf;

use dirt_core::store::{store_path, StoreName};
use dirt_core::{Result, Store};

/// The folder the desktop app and the CLI keep their stores in:
/// `<data dir>/dirt`.
pub fn data_folder() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| panic!("Failed to resolve desktop data directory"))
        .join("dirt")
}

/// Opens the store that holds the notes kept on this device only, creating
/// it on first run. A build with no server uses nothing else.
pub fn open_local_store() -> Result<Store> {
    Store::open(store_path(&data_folder(), &StoreName::Local))
}
