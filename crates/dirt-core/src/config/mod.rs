//! Shared bootstrap/config helpers used by desktop, mobile, and CLI.

use std::time::Duration;

use serde::{Deserialize, Serialize};

const BOOTSTRAP_SCHEMA_VERSION: u32 = 1;
const CLIENT_BOOTSTRAP_HTTP_TIMEOUT_SECS: u64 = 4;
const MANAGED_BOOTSTRAP_HTTP_TIMEOUT_SECS: u64 = 5;

/// Shared client bootstrap configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BootstrapConfig {
    #[serde(default)]
    pub bootstrap_manifest_url: Option<String>,
    #[serde(default)]
    pub supabase_url: Option<String>,
    #[serde(default)]
    pub supabase_anon_key: Option<String>,
    #[serde(default)]
    pub turso_sync_token_endpoint: Option<String>,
    #[serde(default)]
    pub dirt_api_base_url: Option<String>,
}

impl BootstrapConfig {
    /// Returns the managed API base URL for authenticated media operations.
    #[must_use]
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

/// Normalized managed bootstrap payload used by CLI profile setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedBootstrapConfig {
    pub supabase_url: String,
    pub supabase_anon_key: String,
    pub api_base_url: String,
    pub sync_token_endpoint: Option<String>,
    pub managed_sync: bool,
    pub managed_media: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManagedBootstrapManifest {
    schema_version: u32,
    manifest_version: String,
    supabase_url: String,
    supabase_anon_key: String,
    api_base_url: String,
    #[serde(default)]
    turso_sync_token_endpoint: Option<String>,
    feature_flags: ManagedFeatureFlags,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManagedFeatureFlags {
    managed_sync: bool,
    managed_media: bool,
}

/// Normalize optional text by trimming and discarding empty values.
#[must_use]
pub fn normalize_text_option(value: Option<String>) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Resolve runtime bootstrap config by fetching bootstrap manifest when configured.
pub async fn resolve_bootstrap_config(fallback: BootstrapConfig) -> BootstrapConfig {
    let Some(manifest_url) = normalize_text_option(fallback.bootstrap_manifest_url.clone()) else {
        return fallback;
    };

    match fetch_bootstrap_manifest_with_timeout(&manifest_url, CLIENT_BOOTSTRAP_HTTP_TIMEOUT_SECS)
        .await
    {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(
                "Failed to resolve runtime bootstrap from {}: {}. Falling back to embedded config.",
                manifest_url,
                error
            );
            fallback
        }
    }
}

/// Fetch and parse a managed bootstrap endpoint into client runtime config.
pub async fn fetch_bootstrap_manifest(url: &str) -> Result<BootstrapConfig, String> {
    fetch_bootstrap_manifest_with_timeout(url, CLIENT_BOOTSTRAP_HTTP_TIMEOUT_SECS).await
}

/// Parse managed bootstrap JSON for client runtime usage.
pub fn parse_bootstrap_manifest(
    payload: &str,
    manifest_url: &str,
) -> Result<BootstrapConfig, String> {
    let manifest: ManagedBootstrapManifest = serde_json::from_str(payload)
        .map_err(|error| format!("invalid bootstrap manifest JSON: {error}"))?;
    manifest.into_client_config(manifest_url)
}

/// Fetch and parse managed bootstrap endpoint into CLI managed config.
pub async fn fetch_managed_bootstrap_manifest(url: &str) -> Result<ManagedBootstrapConfig, String> {
    let body = fetch_bootstrap_payload(url, MANAGED_BOOTSTRAP_HTTP_TIMEOUT_SECS).await?;
    parse_managed_bootstrap_manifest(&body)
}

/// Parse managed bootstrap JSON into normalized managed config.
pub fn parse_managed_bootstrap_manifest(payload: &str) -> Result<ManagedBootstrapConfig, String> {
    let manifest: ManagedBootstrapManifest = serde_json::from_str(payload)
        .map_err(|error| format!("invalid bootstrap manifest JSON: {error}"))?;
    manifest.into_managed_config()
}

async fn fetch_bootstrap_manifest_with_timeout(
    url: &str,
    timeout_secs: u64,
) -> Result<BootstrapConfig, String> {
    let body = fetch_bootstrap_payload(url, timeout_secs).await?;
    parse_bootstrap_manifest(&body, url)
}

async fn fetch_bootstrap_payload(url: &str, timeout_secs: u64) -> Result<String, String> {
    let url = normalize_required_http_url(url.to_string(), "bootstrap_url")?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|error| format!("failed to build bootstrap HTTP client: {error}"))?;

    let response = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| format!("bootstrap request failed: {error}"))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "bootstrap endpoint returned HTTP {status}: {}",
            compact_text(&body)
        ));
    }

    response
        .text()
        .await
        .map_err(|error| format!("failed to read bootstrap response body: {error}"))
}

impl ManagedBootstrapManifest {
    fn into_client_config(self, manifest_url: &str) -> Result<BootstrapConfig, String> {
        if self.schema_version != BOOTSTRAP_SCHEMA_VERSION {
            return Err(format!(
                "unsupported bootstrap schema_version {} (expected {})",
                self.schema_version, BOOTSTRAP_SCHEMA_VERSION
            ));
        }
        if self.manifest_version.trim().is_empty() {
            return Err("bootstrap manifest_version must not be empty".to_string());
        }

        let supabase_url = normalize_required_http_url(self.supabase_url, "supabase_url")?;
        let supabase_anon_key =
            normalize_required_value(self.supabase_anon_key, "supabase_anon_key")?;
        let api_base_url = normalize_required_http_url(self.api_base_url, "api_base_url")?;

        let sync_endpoint = if self.feature_flags.managed_sync {
            match normalize_text_option(self.turso_sync_token_endpoint) {
                Some(endpoint) => Some(normalize_required_http_url(
                    endpoint,
                    "turso_sync_token_endpoint",
                )?),
                None => Some(format!("{api_base_url}/v1/sync/token")),
            }
        } else {
            None
        };

        let api_base_for_clients =
            if self.feature_flags.managed_sync || self.feature_flags.managed_media {
                Some(api_base_url)
            } else {
                None
            };

        Ok(BootstrapConfig {
            bootstrap_manifest_url: Some(manifest_url.to_string()),
            supabase_url: Some(supabase_url),
            supabase_anon_key: Some(supabase_anon_key),
            turso_sync_token_endpoint: sync_endpoint,
            dirt_api_base_url: api_base_for_clients,
        })
    }

    fn into_managed_config(self) -> Result<ManagedBootstrapConfig, String> {
        if self.schema_version != BOOTSTRAP_SCHEMA_VERSION {
            return Err(format!(
                "unsupported schema_version {} (expected {})",
                self.schema_version, BOOTSTRAP_SCHEMA_VERSION
            ));
        }
        if self.manifest_version.trim().is_empty() {
            return Err("manifest_version must not be empty".to_string());
        }

        let supabase_url = normalize_required_http_url(self.supabase_url, "supabase_url")?;
        let supabase_anon_key =
            normalize_required_value(self.supabase_anon_key, "supabase_anon_key")?;
        let api_base_url = normalize_required_http_url(self.api_base_url, "api_base_url")?;

        let sync_token_endpoint = if self.feature_flags.managed_sync {
            match self.turso_sync_token_endpoint {
                Some(endpoint) => Some(normalize_required_http_url(
                    endpoint,
                    "turso_sync_token_endpoint",
                )?),
                None => Some(format!("{api_base_url}/v1/sync/token")),
            }
        } else {
            None
        };

        Ok(ManagedBootstrapConfig {
            supabase_url,
            supabase_anon_key,
            api_base_url,
            sync_token_endpoint,
            managed_sync: self.feature_flags.managed_sync,
            managed_media: self.feature_flags.managed_media,
        })
    }
}

fn normalize_required_value(raw: String, field: &str) -> Result<String, String> {
    normalize_text_option(Some(raw)).ok_or_else(|| format!("bootstrap field '{field}' is required"))
}

fn normalize_required_http_url(raw: String, field: &str) -> Result<String, String> {
    let value = normalize_required_value(raw, field)?;
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(value.trim_end_matches('/').to_string())
    } else {
        Err(format!(
            "bootstrap field '{field}' must include http:// or https://"
        ))
    }
}

fn compact_text(value: &str) -> String {
    value.trim().chars().take(180).collect()
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
    fn managed_api_base_url_falls_back_to_sync_endpoint_prefix() {
        let config = BootstrapConfig {
            turso_sync_token_endpoint: Some("https://api.example.com/v1/sync/token".to_string()),
            ..Default::default()
        };
        assert_eq!(
            config.managed_api_base_url().as_deref(),
            Some("https://api.example.com")
        );
    }

    #[test]
    fn parse_managed_bootstrap_manifest_derives_sync_endpoint() {
        let payload = r#"
        {
          "schema_version": 1,
          "manifest_version": "v1",
          "supabase_url": "https://project.supabase.co",
          "supabase_anon_key": "anon",
          "api_base_url": "https://api.example.com",
          "feature_flags": {
            "managed_sync": true,
            "managed_media": false
          }
        }
        "#;

        let parsed = parse_managed_bootstrap_manifest(payload).expect("manifest parse");
        assert_eq!(
            parsed.sync_token_endpoint.as_deref(),
            Some("https://api.example.com/v1/sync/token")
        );
        assert!(!parsed.managed_media);
    }
}
