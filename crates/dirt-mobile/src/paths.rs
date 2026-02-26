//! Mobile filesystem path helpers.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

static DIRT_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Shared writable app data directory for Dirt mobile.
#[must_use]
pub fn dirt_data_dir() -> PathBuf {
    DIRT_DATA_DIR.get_or_init(resolve_dirt_data_dir).clone()
}

fn resolve_dirt_data_dir() -> PathBuf {
    let candidates = dirt_data_dir_candidates();
    let selected = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir().join("dirt"));

    tracing::info!("Resolved mobile data directory: {}", selected.display());
    selected
}

/// Returns all verified writable Dirt data directories in priority order.
#[must_use]
pub fn dirt_data_dir_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for base in candidate_base_dirs() {
        let candidate = base.join("dirt");
        if ensure_writable_dir(&candidate) && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }

    candidates
}

fn candidate_base_dirs() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("DIRT_DATA_DIR").map(PathBuf::from) {
        candidates.push(path);
    }
    if let Some(path) = std::env::var_os("TMPDIR").map(PathBuf::from) {
        candidates.push(path);
    }
    if let Some(path) = std::env::var_os("TEMP").map(PathBuf::from) {
        candidates.push(path);
    }
    if let Some(path) = std::env::var_os("TMP").map(PathBuf::from) {
        candidates.push(path);
    }
    if let Some(path) = std::env::var_os("HOME").map(PathBuf::from) {
        candidates.push(path.clone());
        candidates.push(path.join(".local").join("share"));
    }
    if let Some(path) = dirs::data_local_dir() {
        candidates.push(path);
    }
    if let Some(path) = dirs::data_dir() {
        candidates.push(path);
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        candidates.push(path);
    }
    candidates.push(std::env::temp_dir());
    if let Ok(path) = std::env::current_dir() {
        candidates.push(path);
    }
    candidates
}

fn ensure_writable_dir(path: &PathBuf) -> bool {
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }

    let test_file = path.join(".dirt-write-test");
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&test_file)
    else {
        return false;
    };

    if file.write_all(b"ok").is_err() {
        let _ = std::fs::remove_file(&test_file);
        return false;
    }

    let _ = std::fs::remove_file(test_file);
    true
}
