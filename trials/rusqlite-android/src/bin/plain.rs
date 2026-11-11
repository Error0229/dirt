//! The trial with no app around it, to see whether plain `cargo build` links
//! rusqlite's bundled SQLite for Android.
//!
//! ```text
//! cargo build --target x86_64-linux-android --bin plain
//! adb push target/x86_64-linux-android/debug/plain /data/local/tmp/
//! adb shell /data/local/tmp/plain /data/local/tmp/dirt-trial
//! ```

use std::path::PathBuf;

use rusqlite_android_trial::{data_folder, run_trial};

fn main() {
    let folder = std::env::args()
        .nth(1)
        .map_or_else(data_folder, PathBuf::from);
    let report = run_trial(&folder);
    print!("{}", report.to_text());
    if !report.passed() {
        std::process::exit(1);
    }
}
