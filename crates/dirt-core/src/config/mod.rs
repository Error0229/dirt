//! Bootstrap configuration for client apps.
//!
//! Provides a unified `BootstrapConfig` struct used by desktop, mobile, and CLI
//! to discover Supabase auth, Turso sync, and media API endpoints.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::util::{compact_text, is_http_url, normalize_text_option};

const BOOTSTRAP_SCHEMA_VERSION: u32 = 1;
const BOOTSTRAP_HTTP_TIMEOUT_SECS: u64 = 4;

/// Build-provisioned client configuration.
///
/// These values are safe-to-ship public endpoints/keys required to bootstrap
/// auth, sync, and media flows. Secret credentials must never be stored here.
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
    /// Returns the managed API base URL for authenticated operations.
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

/// Resolve runtime bootstrap config by fetching the manifest URL.
///
/// If `bootstrap_manifest_url` is set, fetch/parse/validation failures are
/// returned as errors instead of falling back to embedded values.
pub async fn resolve_bootstrap_config(
    fallback: BootstrapConfig,
) -> Result<BootstrapConfig, String> {
    let Some(manifest_url) = normalize_text_option(fallback.bootstrap_manifest_url.clone()) else {
        return Ok(fallback);
    };

    fetch_bootstrap_manifest(&manifest_url).await
}

/// Parse a bootstrap manifest from a raw JSON payload.
///
/// Public for testability — callers can exercise parsing without network access.
pub fn parse_bootstrap_manifest(
    payload: &str,
    manifest_url: &str,
) -> Result<BootstrapConfig, String> {
    let manifest: ManagedBootstrapManifest = serde_json::from_str(payload)
        .map_err(|error| format!("invalid bootstrap manifest JSON: {error}"))?;
    manifest.into_runtime_config(manifest_url)
}

// ---------------------------------------------------------------------------
// Private
// ---------------------------------------------------------------------------

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

impl ManagedBootstrapManifest {
    fn into_runtime_config(self, manifest_url: &str) -> Result<BootstrapConfig, String> {
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
}

async fn fetch_bootstrap_manifest(url: &str) -> Result<BootstrapConfig, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(BOOTSTRAP_HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|error| format!("failed to build bootstrap HTTP client: {error}"))?;

    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| format!("bootstrap request failed: {error}"))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|error| format!("failed to read bootstrap error response body: {error}"))?;
        return Err(format!(
            "bootstrap endpoint returned HTTP {status}: {}",
            compact_text(&body)
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read bootstrap response body: {error}"))?;
    parse_bootstrap_manifest(&body, url)
}

fn normalize_required_value(raw: String, field: &str) -> Result<String, String> {
    normalize_text_option(Some(raw)).ok_or_else(|| format!("bootstrap field '{field}' is required"))
}

fn normalize_required_http_url(raw: String, field: &str) -> Result<String, String> {
    let value = normalize_required_value(raw, field)?;
    if is_http_url(&value) {
        Ok(value.trim_end_matches('/').to_string())
    } else {
        Err(format!(
            "bootstrap field '{field}' must include http:// or https://"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_manifest_rejects_unknown_fields() {
        let payload = r#"
        {
          "schema_version": 1,
          "manifest_version": "v1",
          "supabase_url": "https://project.supabase.co",
          "supabase_anon_key": "anon",
          "api_base_url": "https://api.example.com",
          "feature_flags": {
            "managed_sync": true,
            "managed_media": true,
            "unexpected": true
          }
        }
        "#;

        let error =
            parse_bootstrap_manifest(payload, "https://api.example.com/v1/bootstrap").unwrap_err();
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn parse_manifest_rejects_invalid_schema_version() {
        let payload = r#"
        {
          "schema_version": 9,
          "manifest_version": "v1",
          "supabase_url": "https://project.supabase.co",
          "supabase_anon_key": "anon",
          "api_base_url": "https://api.example.com",
          "feature_flags": {
            "managed_sync": true,
            "managed_media": true
          }
        }
        "#;

        let error =
            parse_bootstrap_manifest(payload, "https://api.example.com/v1/bootstrap").unwrap_err();
        assert!(error.contains("schema_version"));
    }

    #[test]
    fn parse_manifest_derives_sync_endpoint_when_missing() {
        let payload = r#"
        {
          "schema_version": 1,
          "manifest_version": "v2",
          "supabase_url": "https://project.supabase.co",
          "supabase_anon_key": "anon",
          "api_base_url": "https://api.example.com",
          "feature_flags": {
            "managed_sync": true,
            "managed_media": false
          }
        }
        "#;

        let parsed = parse_bootstrap_manifest(payload, "https://api.example.com/v1/bootstrap")
            .expect("manifest should parse");
        assert_eq!(
            parsed.turso_sync_token_endpoint.as_deref(),
            Some("https://api.example.com/v1/sync/token")
        );
        assert_eq!(
            parsed.dirt_api_base_url.as_deref(),
            Some("https://api.example.com")
        );
    }
}
