//! Runtime configuration handling for mobile.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::{Path, PathBuf};

use dirt_core::db::SyncConfig;
use dirt_core::Result;
use serde::{Deserialize, Serialize};

const RUNTIME_CONFIG_FILE: &str = "mobile-config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncConfigSource {
    RuntimeSettings,
    EnvironmentFallback,
    None,
}

#[derive(Debug, Clone)]
pub struct ResolvedSyncConfig {
    pub sync_config: Option<SyncConfig>,
    pub source: SyncConfigSource,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileRuntimeConfig {
    #[serde(default)]
    pub turso_database_url: Option<String>,
    #[serde(default)]
    pub turso_auth_token: Option<String>,
}

impl MobileRuntimeConfig {
    pub fn from_raw(url: Option<String>, auth_token: Option<String>) -> Self {
        Self {
            turso_database_url: normalize_text_option(url),
            turso_auth_token: normalize_text_option(auth_token),
        }
    }

    pub const fn has_sync_config(&self) -> bool {
        self.turso_database_url.is_some() && self.turso_auth_token.is_some()
    }

    pub fn sync_config(&self) -> Option<SyncConfig> {
        parse_sync_config(
            self.turso_database_url.clone(),
            self.turso_auth_token.clone(),
        )
    }
}

pub fn default_runtime_config_path() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dirt")
        .join(RUNTIME_CONFIG_FILE)
}

pub fn load_runtime_config() -> MobileRuntimeConfig {
    load_runtime_config_from_path(&default_runtime_config_path())
}

pub fn load_runtime_config_from_path(path: &Path) -> MobileRuntimeConfig {
    if !path.exists() {
        return MobileRuntimeConfig::default();
    }

    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<MobileRuntimeConfig>(&content) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(
                    "Failed to parse mobile runtime config at {}: {}",
                    path.display(),
                    error
                );
                MobileRuntimeConfig::default()
            }
        },
        Err(error) => {
            tracing::warn!(
                "Failed to read mobile runtime config at {}: {}",
                path.display(),
                error
            );
            MobileRuntimeConfig::default()
        }
    }
}

pub fn save_runtime_config(config: &MobileRuntimeConfig) -> Result<()> {
    save_runtime_config_to_path(config, &default_runtime_config_path())
}

pub fn save_runtime_config_to_path(config: &MobileRuntimeConfig, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let normalized = MobileRuntimeConfig::from_raw(
        config.turso_database_url.clone(),
        config.turso_auth_token.clone(),
    );
    let content = serde_json::to_string_pretty(&normalized)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn resolve_sync_config() -> ResolvedSyncConfig {
    let runtime_config = load_runtime_config();
    if let Some(sync_config) = runtime_config.sync_config() {
        return ResolvedSyncConfig {
            sync_config: Some(sync_config),
            source: SyncConfigSource::RuntimeSettings,
        };
    }

    if let Some(sync_config) = parse_sync_config(
        std::env::var("TURSO_DATABASE_URL").ok(),
        std::env::var("TURSO_AUTH_TOKEN").ok(),
    ) {
        return ResolvedSyncConfig {
            sync_config: Some(sync_config),
            source: SyncConfigSource::EnvironmentFallback,
        };
    }

    ResolvedSyncConfig {
        sync_config: None,
        source: SyncConfigSource::None,
    }
}

pub fn parse_sync_config(url: Option<String>, auth_token: Option<String>) -> Option<SyncConfig> {
    let url = normalize_text_option(url)?;
    let auth_token = normalize_text_option(auth_token)?;
    Some(SyncConfig::new(url, auth_token))
}

fn normalize_text_option(value: Option<String>) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sync_config_requires_both_values() {
        assert!(parse_sync_config(None, Some("token".to_string())).is_none());
        assert!(parse_sync_config(Some("libsql://db.turso.io".to_string()), None).is_none());
    }

    #[test]
    fn parse_sync_config_rejects_empty_values() {
        assert!(parse_sync_config(Some("   ".to_string()), Some("token".to_string())).is_none());
        assert!(parse_sync_config(
            Some("libsql://db.turso.io".to_string()),
            Some("   ".to_string()),
        )
        .is_none());
    }

    #[test]
    fn parse_sync_config_accepts_valid_values() {
        let config = parse_sync_config(
            Some(" libsql://db.turso.io ".to_string()),
            Some(" token ".to_string()),
        )
        .unwrap();

        assert_eq!(config.url.as_deref(), Some("libsql://db.turso.io"));
        assert_eq!(config.auth_token.as_deref(), Some("token"));
        assert!(config.is_configured());
    }

    #[test]
    fn save_and_load_runtime_config_roundtrip() {
        let test_dir = std::env::temp_dir().join(format!(
            "dirt-mobile-config-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let config_path = test_dir.join("mobile-config.json");

        let config = MobileRuntimeConfig::from_raw(
            Some(" libsql://runtime.turso.io ".to_string()),
            Some(" runtime-token ".to_string()),
        );
        save_runtime_config_to_path(&config, &config_path).unwrap();

        let loaded = load_runtime_config_from_path(&config_path);
        assert_eq!(
            loaded.turso_database_url.as_deref(),
            Some("libsql://runtime.turso.io")
        );
        assert_eq!(loaded.turso_auth_token.as_deref(), Some("runtime-token"));

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(test_dir);
    }
}
