//! Desktop bootstrap configuration loaded from build-time generated JSON.

use serde::{Deserialize, Serialize};

/// Build-provisioned client configuration embedded into desktop binaries.
///
/// These values are safe-to-ship public endpoints/keys required to bootstrap
/// auth, sync, and media flows. Secret credentials must never be stored here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopBootstrapConfig {
    #[serde(default)]
    pub supabase_url: Option<String>,
    #[serde(default)]
    pub supabase_anon_key: Option<String>,
    #[serde(default)]
    pub turso_sync_token_endpoint: Option<String>,
    #[serde(default)]
    pub dirt_api_base_url: Option<String>,
}

/// Loads the generated desktop bootstrap JSON from `OUT_DIR`.
///
/// If parsing fails, this logs a warning and returns a default empty config so
/// the app can continue running in local-only mode.
pub fn load_bootstrap_config() -> DesktopBootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/desktop-bootstrap.json"));
    serde_json::from_str(raw).unwrap_or_else(|error| {
        tracing::warn!("Failed to parse desktop bootstrap config: {}", error);
        DesktopBootstrapConfig::default()
    })
}

/// Normalizes optional text config by trimming whitespace and removing empties.
///
/// Returns `None` when the input is `None` or the trimmed value is empty.
pub fn normalize_text_option(value: Option<String>) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

impl DesktopBootstrapConfig {
    /// Returns the managed API base URL for authenticated media operations.
    ///
    /// Prefers `dirt_api_base_url` when present; otherwise derives the base URL
    /// from `turso_sync_token_endpoint` by stripping `/v1/sync/token`.
    pub fn managed_api_base_url(&self) -> Option<String> {
        if let Some(url) = normalize_text_option(self.dirt_api_base_url.clone()) {
            return Some(url);
        }

        let endpoint = normalize_text_option(self.turso_sync_token_endpoint.clone())?;
        endpoint
            .strip_suffix("/v1/sync/token")
            .map(std::string::ToString::to_string)
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
    fn normalize_text_option_trims_value() {
        assert_eq!(
            normalize_text_option(Some(" https://example.com ".to_string())),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn managed_api_base_url_falls_back_to_sync_endpoint_prefix() {
        let config = DesktopBootstrapConfig {
            turso_sync_token_endpoint: Some("https://api.example.com/v1/sync/token".to_string()),
            ..Default::default()
        };
        assert_eq!(
            config.managed_api_base_url().as_deref(),
            Some("https://api.example.com")
        );
    }
}
