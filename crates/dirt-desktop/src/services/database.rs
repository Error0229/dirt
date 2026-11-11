//! Opens the desktop app's local note store.

use dirt_core::store::{store_path, StoreName};
use dirt_core::{Result, Store};

/// Opens the store that holds the notes kept on this device, creating it on
/// first run. The file is `<data dir>/dirt/stores/local/notes.db`.
pub fn open_local_store() -> Result<Store> {
    let data_folder = dirs::data_dir()
        .unwrap_or_else(|| panic!("Failed to resolve desktop data directory"))
        .join("dirt");
    Store::open(store_path(&data_folder, &StoreName::Local))
}
