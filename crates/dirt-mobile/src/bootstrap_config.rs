//! Mobile bootstrap configuration loaded from build-time generated JSON.
//!
//! `build.rs` writes `mobile-bootstrap.json` to `OUT_DIR` containing the
//! `DIRT_API_BASE_URL` value resolved from the workspace `.env.client`
//! (with `localhost`/`127.0.0.1` rewritten to `10.0.2.2` for the
//! Android emulator). We `include_str!` it at compile time so a
//! packaged APK works without an env var at launch — the bearer token
//! still has to come from the runtime environment.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

pub use dirt_core::config::BootstrapConfig;

/// Load the build-time mobile bootstrap manifest. Always succeeds —
/// missing fields just yield `None` on the corresponding Option.
#[must_use]
pub fn load_bootstrap_config() -> BootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/mobile-bootstrap.json"));
    serde_json::from_str(raw)
        .unwrap_or_else(|error| panic!("Failed to parse mobile bootstrap config: {error}"))
}
