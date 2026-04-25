//! Bootstrap configuration for client apps.
//!
//! `BootstrapConfig` carries the safe-to-ship public endpoints clients
//! need at startup. Post-Supabase the only meaningful field is
//! `dirt_api_base_url`; bearer-token secrets live in the OS keychain
//! (clients) or in `DIRT_SERVER_TOKEN` (server), never here.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::util::{compact_text, is_http_url, normalize_text_option};

const BOOTSTRAP_SCHEMA_VERSION: u32 = 2;
const BOOTSTRAP_HTTP_TIMEOUT_SECS: u64 = 4;

/// Build-provisioned client configuration.
///
/// Safe-to-ship public endpoints required to bootstrap the API client.
/// Secret credentials must never be stored here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BootstrapConfig {
    #[serde(default)]
    pub bootstrap_manifest_url: Option<String>,
    #[serde(default)]
    pub dirt_api_base_url: Option<String>,
}

impl BootstrapConfig {
    /// Returns the dirt-api base URL when configured.
    pub fn managed_api_base_url(&self) -> Option<String> {
        normalize_text_option(self.dirt_api_base_url.clone())
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
    api_base_url: String,
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

        let api_base_url = normalize_required_http_url(self.api_base_url, "api_base_url")?;

        Ok(BootstrapConfig {
            bootstrap_manifest_url: Some(manifest_url.to_string()),
            dirt_api_base_url: Some(api_base_url),
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
    fn managed_api_base_url_returns_configured_value() {
        let config = BootstrapConfig {
            dirt_api_base_url: Some("https://api.example.com".to_string()),
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
          "schema_version": 2,
          "manifest_version": "v1",
          "api_base_url": "https://api.example.com",
          "supabase_url": "https://leftover.example.com"
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
          "api_base_url": "https://api.example.com"
        }
        "#;

        let error =
            parse_bootstrap_manifest(payload, "https://api.example.com/v1/bootstrap").unwrap_err();
        assert!(error.contains("schema_version"));
    }

    #[test]
    fn parse_manifest_returns_api_base_url() {
        let payload = r#"
        {
          "schema_version": 2,
          "manifest_version": "v2",
          "api_base_url": "https://api.example.com"
        }
        "#;

        let parsed = parse_bootstrap_manifest(payload, "https://api.example.com/v1/bootstrap")
            .expect("manifest should parse");
        assert_eq!(
            parsed.dirt_api_base_url.as_deref(),
            Some("https://api.example.com")
        );
    }
}
