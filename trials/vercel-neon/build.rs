//! Records the compiler version, so `/healthz` can report which Rust the
//! Vercel builder used.

use std::process::Command;

fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let version = Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map_or_else(|| "unknown".to_owned(), |text| text.trim().to_owned());
    println!("cargo:rustc-env=TRIAL_RUSTC_VERSION={version}");
    println!("cargo:rerun-if-changed=build.rs");
    // `sqlx::migrate!` embeds this folder at compile time.
    println!("cargo:rerun-if-changed=migrations");
}
