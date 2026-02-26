//! Persistent CLI profile configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "cli-config.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CliProfilesConfig {
    #[serde(default = "default_config_version")]
    pub version: u32,
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, CliProfile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CliProfile {
    #[serde(default)]
    pub supabase_url: Option<String>,
    #[serde(default)]
    pub supabase_anon_key: Option<String>,
    #[serde(default)]
    pub turso_sync_token_endpoint: Option<String>,
    #[serde(default)]
    pub dirt_api_base_url: Option<String>,
}

const fn default_config_version() -> u32 {
    1
}

pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| panic!("Failed to resolve CLI config directory"))
        .join("dirt")
        .join(CONFIG_FILE_NAME)
}

pub fn normalize_text_option(value: Option<String>) -> Option<String> {
    dirt_core::util::normalize_text_option(value)
}

pub fn normalize_profile_name(value: Option<&str>) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub fn is_http_url(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("https://") || value.starts_with("http://")
}

impl CliProfilesConfig {
    pub fn load() -> Result<Self, String> {
        Self::load_from_path(&default_config_path())
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = std::fs::read_to_string(path)
            .map_err(|error| format!("Failed to read config at {}: {}", path.display(), error))?;
        let mut config = serde_json::from_str::<Self>(&raw)
            .map_err(|error| format!("Failed to parse config at {}: {}", path.display(), error))?;
        config.normalize();
        Ok(config)
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        let path = default_config_path();
        self.save_to_path(&path)?;
        Ok(path)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed to create config directory {}: {}",
                    parent.display(),
                    error
                )
            })?;
        }

        let mut normalized = self.clone();
        normalized.normalize();
        let serialized = serde_json::to_string_pretty(&normalized)
            .map_err(|error| format!("Failed to serialize config: {error}"))?;
        std::fs::write(path, serialized)
            .map_err(|error| format!("Failed to write config at {}: {}", path.display(), error))
    }

    pub fn resolve_profile_name(&self, explicit: Option<&str>) -> String {
        if let Some(profile) = normalize_profile_name(explicit) {
            return profile;
        }
        if let Some(profile) = normalize_profile_name(std::env::var("DIRT_PROFILE").ok().as_deref())
        {
            return profile;
        }
        if let Some(profile) = normalize_profile_name(self.active_profile.as_deref()) {
            return profile;
        }
        "default".to_string()
    }

    pub fn profile(&self, name: &str) -> Option<&CliProfile> {
        self.profiles.get(name)
    }

    pub fn profile_mut_or_default(&mut self, name: &str) -> &mut CliProfile {
        self.profiles.entry(name.to_string()).or_default()
    }

    fn normalize(&mut self) {
        self.active_profile = normalize_profile_name(self.active_profile.as_deref());
        for profile in self.profiles.values_mut() {
            profile.normalize();
        }
    }
}

impl CliProfile {
    pub fn managed_sync_endpoint(&self) -> Option<String> {
        normalize_text_option(self.turso_sync_token_endpoint.clone())
    }

    pub fn supabase_url(&self) -> Option<String> {
        normalize_text_option(self.supabase_url.clone())
    }

    pub fn supabase_anon_key(&self) -> Option<String> {
        normalize_text_option(self.supabase_anon_key.clone())
    }

    fn normalize(&mut self) {
        self.supabase_url = normalize_text_option(self.supabase_url.clone());
        self.supabase_anon_key = normalize_text_option(self.supabase_anon_key.clone());
        self.turso_sync_token_endpoint =
            normalize_text_option(self.turso_sync_token_endpoint.clone());
        self.dirt_api_base_url = normalize_text_option(self.dirt_api_base_url.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_text_option_rejects_empty() {
        assert_eq!(normalize_text_option(None), None);
        assert_eq!(normalize_text_option(Some("   ".to_string())), None);
    }

    #[test]
    fn normalize_profile_name_rejects_empty() {
        assert_eq!(normalize_profile_name(None), None);
        assert_eq!(normalize_profile_name(Some(" ")), None);
    }

    #[test]
    fn config_roundtrip_preserves_profiles() {
        let path = std::env::temp_dir().join(format!(
            "dirt-cli-config-test-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));

        let mut config = CliProfilesConfig {
            version: 1,
            active_profile: Some("default".to_string()),
            profiles: BTreeMap::new(),
        };
        config.profiles.insert(
            "default".to_string(),
            CliProfile {
                supabase_url: Some(" https://project.supabase.co ".to_string()),
                supabase_anon_key: Some(" anon-key ".to_string()),
                turso_sync_token_endpoint: Some(
                    " https://api.example.com/v1/sync/token ".to_string(),
                ),
                dirt_api_base_url: None,
            },
        );

        config.save_to_path(&path).unwrap();
        let loaded = CliProfilesConfig::load_from_path(&path).unwrap();
        let profile = loaded.profiles.get("default").unwrap();
        assert_eq!(
            profile.supabase_url.as_deref(),
            Some("https://project.supabase.co")
        );
        assert_eq!(profile.supabase_anon_key.as_deref(), Some("anon-key"));
        assert_eq!(
            profile.turso_sync_token_endpoint.as_deref(),
            Some("https://api.example.com/v1/sync/token")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn resolve_profile_name_prefers_explicit_then_active() {
        let config = CliProfilesConfig {
            version: 1,
            active_profile: Some("work".to_string()),
            profiles: BTreeMap::new(),
        };
        assert_eq!(config.resolve_profile_name(Some("mobile")), "mobile");
        assert_eq!(config.resolve_profile_name(None), "work");
    }
}
