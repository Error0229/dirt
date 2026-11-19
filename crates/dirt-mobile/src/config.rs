//! Runtime configuration handling for mobile.
//!
//! Just the data directory plumbing. Nothing secret is kept here; the
//! account's keys live in the device's secure storage (see
//! `services::secure_storage`).
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::PathBuf;

/// The app's data folder: `dirt` in the app's own files folder.
///
/// On Android it is asked of Android (`Context.getFilesDir`), once, so that
/// it is the folder the capture sheet and its job use too, which Kotlin
/// gives them the same way: for a second user of the phone, or a work
/// profile, the files folder is not under `/data/user/0`. Worked out from
/// the process only if Android could not be asked.
#[cfg(target_os = "android")]
pub fn default_mobile_data_directory() -> PathBuf {
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            crate::services::activity::files_dir().map_or_else(
                || {
                    tracing::warn!("Android did not say where the app's files are");
                    worked_out_data_directory()
                },
                |files| files.join("dirt"),
            )
        })
        .clone()
}

/// The app's data folder, away from Android: for the tests.
#[cfg(not(target_os = "android"))]
pub fn default_mobile_data_directory() -> PathBuf {
    worked_out_data_directory()
}

fn worked_out_data_directory() -> PathBuf {
    dirs::data_local_dir().or_else(dirs::data_dir).map_or_else(
        || {
            let fallback = fallback_mobile_data_directory();
            tracing::warn!(
                "Failed to resolve mobile data directory from OS defaults; falling back to {}",
                fallback.display()
            );
            fallback
        },
        |dir| dir.join("dirt"),
    )
}

#[cfg(target_os = "android")]
fn fallback_mobile_data_directory() -> PathBuf {
    android_process_name().map_or_else(
        || std::env::temp_dir().join("dirt"),
        |process_name| {
            PathBuf::from("/data/user/0")
                .join(process_name)
                .join("files")
                .join("dirt")
        },
    )
}

#[cfg(not(target_os = "android"))]
fn fallback_mobile_data_directory() -> PathBuf {
    std::env::temp_dir().join("dirt")
}

#[cfg(target_os = "android")]
fn android_process_name() -> Option<String> {
    let cmdline = std::fs::read("/proc/self/cmdline").ok()?;
    parse_cmdline_process_name(&cmdline)
}

fn parse_cmdline_process_name(cmdline: &[u8]) -> Option<String> {
    let end = cmdline
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(cmdline.len());
    let raw = std::str::from_utf8(&cmdline[..end]).ok()?;
    let normalized = raw.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cmdline_process_name_handles_terminator() {
        let name = parse_cmdline_process_name(b"com.example.DirtMobile\0ignored");
        assert_eq!(name.as_deref(), Some("com.example.DirtMobile"));
    }

    #[test]
    fn parse_cmdline_process_name_rejects_blank_values() {
        assert!(parse_cmdline_process_name(b"   \0").is_none());
        assert!(parse_cmdline_process_name(&[]).is_none());
    }
}
