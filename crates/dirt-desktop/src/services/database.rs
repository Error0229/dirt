//! Where the desktop app keeps its note stores.

use std::path::PathBuf;

use dirt_core::store::StoreName;
use dirt_core::{Result, Store};

/// The folder the desktop app and the CLI keep their stores in:
/// `DIRT_DATA_DIR` if it is set, which is for trying things out beside a
/// real installation, and otherwise `<data dir>/dirt`.
pub fn data_folder() -> PathBuf {
    if let Some(folder) = std::env::var_os("DIRT_DATA_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(folder);
    }
    dirs::data_dir()
        .unwrap_or_else(|| panic!("Failed to resolve desktop data directory"))
        .join("dirt")
}

/// Opens the store that holds the notes kept on this device only, creating
/// it on first run. A build with no server uses nothing else.
pub fn open_local_store() -> Result<Store> {
    Store::open_in(&data_folder(), &StoreName::Local)
}
