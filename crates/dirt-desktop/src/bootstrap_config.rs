//! Desktop bootstrap configuration loaded from build-time generated JSON.
//!
//! Re-exports the shared `BootstrapConfig` from dirt-core and provides
//! the desktop-specific `load_bootstrap_config` function that reads the
//! embedded build-time JSON.

pub use dirt_core::config::{resolve_bootstrap_config, BootstrapConfig};

/// Loads the generated desktop bootstrap JSON from `OUT_DIR`.
pub fn load_bootstrap_config() -> BootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/desktop-bootstrap.json"));
    serde_json::from_str(raw)
        .unwrap_or_else(|error| panic!("Failed to parse desktop bootstrap config: {error}"))
}
