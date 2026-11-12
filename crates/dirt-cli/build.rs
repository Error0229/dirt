//! Gives the CLI the server address it is built with.
//!
//! The CLI and the desktop app on one computer are one device, so they have
//! to name one server. This reads what the desktop app's build script reads:
//! `DIRT_DESKTOP_API_BASE_URL`, or else `DIRT_API_BASE_URL`, from the
//! environment of the build or from `.env.client` at the workspace root.
//! `DIRT_API_BASE_URL` in the environment when the CLI runs comes before
//! either (`dirt_core::config::ServerAddress`).

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=DIRT_DESKTOP_API_BASE_URL");
    println!("cargo:rerun-if-env-changed=DIRT_API_BASE_URL");
    println!("cargo:rerun-if-changed=../../.env.client");

    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let client_env = manifest_dir.join("..").join("..").join(".env.client");
    if client_env.exists() {
        // Values already in the environment win over the file's.
        let _ = dotenvy::from_path(client_env);
    }

    let address = ["DIRT_DESKTOP_API_BASE_URL", "DIRT_API_BASE_URL"]
        .into_iter()
        .filter_map(|name| env::var(name).ok())
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty());
    if let Some(address) = address {
        println!("cargo:rustc-env=DIRT_BUILT_IN_API_BASE_URL={address}");
    }
}
