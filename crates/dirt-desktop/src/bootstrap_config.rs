//! The server address the desktop app was built with.
//!
//! `build.rs` writes `DIRT_API_BASE_URL` from `.env.client` into
//! `desktop-bootstrap.json`, which is included here. The app passes it to
//! `dirt_core::config::ServerAddress`, which lets the environment come
//! first and gives this computer one name.

pub use dirt_core::config::BootstrapConfig;

/// Loads the generated desktop bootstrap JSON from `OUT_DIR`.
pub fn load_bootstrap_config() -> BootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/desktop-bootstrap.json"));
    serde_json::from_str(raw)
        .unwrap_or_else(|error| panic!("Failed to parse desktop bootstrap config: {error}"))
}
