//! Runtime configuration handling for mobile.
//!
//! Post-Supabase shape: just the data directory plumbing. The sync URL
//! and Turso auth-token persistence that lived here previously are gone
//! along with the embedded-replica sync path; the new ApiClient-driven
//! flow keeps the bearer token in the OS secure-storage layer that lands
//! with the mobile sync worker (next commit).
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::PathBuf;

pub fn default_mobile_data_directory() -> PathBuf {
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
    android_process_name()
        .map(|process_name| {
            PathBuf::from("/data/user/0")
                .join(process_name)
                .join("files")
                .join("dirt")
        })
        .unwrap_or_else(|| std::env::temp_dir().join("dirt"))
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
