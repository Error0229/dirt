//! The server address the Android app was built with.
//!
//! `build.rs` writes `DIRT_MOBILE_API_BASE_URL`, or else
//! `DIRT_API_BASE_URL`, from `.env.client` into `mobile-bootstrap.json`,
//! which is included here, so a packaged APK needs nothing set when it
//! starts. The app passes the address to
//! `dirt_core::config::ServerAddress::on_android`.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

pub use dirt_core::config::BootstrapConfig;

/// Load the build-time mobile bootstrap manifest.
#[must_use]
pub fn load_bootstrap_config() -> BootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/mobile-bootstrap.json"));
    serde_json::from_str(raw)
        .unwrap_or_else(|error| panic!("Failed to parse mobile bootstrap config: {error}"))
}
