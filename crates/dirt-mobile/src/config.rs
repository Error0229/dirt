//! Runtime configuration handling for mobile.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::{Path, PathBuf};

use dirt_core::db::SyncConfig;
use dirt_core::util::normalize_text_option;
use dirt_core::Result;
use serde::{Deserialize, Serialize};

use crate::secret_store;

const RUNTIME_CONFIG_FILE: &str = "mobile-config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncConfigSource {
    RuntimeSettings,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Status of a runtime secret in secure storage.
pub enum SecretStatus {
    /// Secret exists and can be read.
    Present,
    /// Secret is not present.
    Missing,
    /// Secret read failed.
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ResolvedSyncConfig {
    pub sync_config: Option<SyncConfig>,
    pub source: SyncConfigSource,
    /// User-facing warning for partial/invalid sync configuration states.
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileRuntimeConfig {
    #[serde(default)]
    pub turso_database_url: Option<String>,
}

impl MobileRuntimeConfig {
    pub fn from_raw(url: Option<String>) -> Self {
        Self {
            turso_database_url: normalize_text_option(url),
        }
    }

    pub const fn has_sync_url(&self) -> bool {
        self.turso_database_url.is_some()
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

    let normalized = MobileRuntimeConfig::from_raw(config.turso_database_url.clone());
    let content = serde_json::to_string_pretty(&normalized)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn resolve_sync_config() -> ResolvedSyncConfig {
    let runtime_config = load_runtime_config();
    let runtime_secret = secret_store::read_secret(secret_store::SECRET_TURSO_AUTH_TOKEN);
    resolve_sync_config_from_sources(runtime_config.turso_database_url, runtime_secret)
}

/// Report secure-storage status for the runtime Turso auth token.
pub fn runtime_turso_token_status() -> SecretStatus {
    match secret_store::read_secret(secret_store::SECRET_TURSO_AUTH_TOKEN) {
        Ok(Some(_)) => SecretStatus::Present,
        Ok(None) => SecretStatus::Missing,
        Err(error) => SecretStatus::Error(error),
    }
}

fn resolve_sync_config_from_sources(
    runtime_url: Option<String>,
    runtime_secret: std::result::Result<Option<String>, String>,
) -> ResolvedSyncConfig {
    let runtime_url_was_set = runtime_url.is_some();
    let runtime_sync_config = parse_sync_config(
        runtime_url,
        runtime_secret.as_ref().ok().and_then(Clone::clone),
    );
    if let Some(sync_config) = runtime_sync_config {
        return ResolvedSyncConfig {
            sync_config: Some(sync_config),
            source: SyncConfigSource::RuntimeSettings,
            warning: None,
        };
    }

    let warning = if runtime_url_was_set {
        match runtime_secret {
            Ok(None) => Some(
                "Turso URL is configured but sync auth token is missing. Sign in and refresh sync settings."
                    .to_string(),
            ),
            Err(error) => Some(format!(
                "Failed to read Turso auth token from secure storage: {error}. Running local-only."
            )),
            Ok(Some(_)) => None,
        }
    } else {
        None
    };

    ResolvedSyncConfig {
        sync_config: None,
        source: SyncConfigSource::None,
        warning,
    }
}

pub fn parse_sync_config(url: Option<String>, auth_token: Option<String>) -> Option<SyncConfig> {
    let url = normalize_text_option(url)?;
    let auth_token = normalize_text_option(auth_token)?;
    Some(SyncConfig::new(url, auth_token))
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

        let config = MobileRuntimeConfig::from_raw(Some(" libsql://runtime.turso.io ".to_string()));
        save_runtime_config_to_path(&config, &config_path).unwrap();

        let loaded = load_runtime_config_from_path(&config_path);
        assert_eq!(
            loaded.turso_database_url.as_deref(),
            Some("libsql://runtime.turso.io")
        );

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(test_dir);
    }

    #[test]
    fn runtime_url_without_secret_yields_warning() {
        let resolved = resolve_sync_config_from_sources(
            Some("libsql://runtime.turso.io".to_string()),
            Ok(None),
        );

        assert_eq!(resolved.source, SyncConfigSource::None);
        assert!(resolved.sync_config.is_none());
        assert!(resolved
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("auth token")));
    }
}
