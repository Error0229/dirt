//! Managed bootstrap manifest client for CLI profile initialization.

use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

use crate::config_profiles::is_http_url;

const BOOTSTRAP_SCHEMA_VERSION: u32 = 1;
const BOOTSTRAP_HTTP_TIMEOUT_SECS: u64 = 5;

/// Managed runtime configuration loaded from the backend bootstrap endpoint.
///
/// These values are public bootstrap fields that allow the CLI to initialize
/// profile auth/sync configuration without user-provided infra secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedBootstrapConfig {
    /// Supabase project URL used for auth flows.
    pub supabase_url: String,
    /// Supabase anon/public key used by client auth requests.
    pub supabase_anon_key: String,
    /// Public Dirt API base URL used for managed backend operations.
    pub api_base_url: String,
    /// Optional explicit managed sync token endpoint.
    pub sync_token_endpoint: Option<String>,
    /// Whether managed sync token exchange is enabled.
    pub managed_sync: bool,
    /// Whether managed media presign flows are enabled.
    pub managed_media: bool,
}

/// Errors returned while fetching or parsing managed bootstrap manifests.
#[derive(Debug, Error)]
pub enum ManagedBootstrapError {
    #[error("Invalid bootstrap URL: {0}")]
    InvalidUrl(String),
    #[error("Bootstrap request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Bootstrap endpoint returned HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("Invalid bootstrap payload: {0}")]
    InvalidPayload(String),
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

/// Fetches and validates a managed bootstrap manifest from the given URL.
///
/// This call enforces schema validation and URL normalization before returning
/// runtime client configuration.
pub async fn fetch_bootstrap_manifest(
    bootstrap_url: &str,
) -> Result<ManagedBootstrapConfig, ManagedBootstrapError> {
    let bootstrap_url = normalize_url(bootstrap_url, "bootstrap_url")?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(BOOTSTRAP_HTTP_TIMEOUT_SECS))
        .build()?;

    let response = client
        .get(&bootstrap_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(ManagedBootstrapError::HttpStatus {
            status,
            body: compact_text(&body),
        });
    }

    let body = response.text().await?;
    parse_bootstrap_manifest(&body)
}

/// Parses a bootstrap manifest JSON payload into runtime configuration.
///
/// Parsing rejects unknown fields and unsupported schema versions.
pub fn parse_bootstrap_manifest(
    payload: &str,
) -> Result<ManagedBootstrapConfig, ManagedBootstrapError> {
    let manifest: ManagedBootstrapManifest = serde_json::from_str(payload)
        .map_err(|error| ManagedBootstrapError::InvalidPayload(error.to_string()))?;
    manifest.into_runtime_config()
}

impl ManagedBootstrapManifest {
    fn into_runtime_config(self) -> Result<ManagedBootstrapConfig, ManagedBootstrapError> {
        if self.schema_version != BOOTSTRAP_SCHEMA_VERSION {
            return Err(ManagedBootstrapError::InvalidPayload(format!(
                "unsupported schema_version {} (expected {})",
                self.schema_version, BOOTSTRAP_SCHEMA_VERSION
            )));
        }
        if self.manifest_version.trim().is_empty() {
            return Err(ManagedBootstrapError::InvalidPayload(
                "manifest_version must not be empty".to_string(),
            ));
        }

        let supabase_url = normalize_url(&self.supabase_url, "supabase_url")?;
        let supabase_anon_key =
            normalize_required_value(&self.supabase_anon_key, "supabase_anon_key")?;
        let api_base_url = normalize_url(&self.api_base_url, "api_base_url")?;

        let sync_token_endpoint = if self.feature_flags.managed_sync {
            match self.turso_sync_token_endpoint {
                Some(endpoint) => Some(normalize_url(&endpoint, "turso_sync_token_endpoint")?),
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

fn normalize_required_value(value: &str, field: &str) -> Result<String, ManagedBootstrapError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(ManagedBootstrapError::InvalidPayload(format!(
            "field '{field}' must not be empty"
        )))
    } else {
        Ok(trimmed.to_string())
    }
}

fn normalize_url(value: &str, field: &str) -> Result<String, ManagedBootstrapError> {
    let value = normalize_required_value(value, field)?;
    if is_http_url(&value) {
        Ok(value.trim_end_matches('/').to_string())
    } else {
        Err(ManagedBootstrapError::InvalidUrl(format!(
            "field '{field}' must include http:// or https://"
        )))
    }
}

fn compact_text(value: &str) -> String {
    value.trim().chars().take(180).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bootstrap_manifest_rejects_unknown_fields() {
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
            "unknown": true
          }
        }
        "#;

        let error = parse_bootstrap_manifest(payload).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn parse_bootstrap_manifest_rejects_invalid_schema() {
        let payload = r#"
        {
          "schema_version": 2,
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

        let error = parse_bootstrap_manifest(payload).unwrap_err();
        assert!(error.to_string().contains("schema_version"));
    }

    #[test]
    fn parse_bootstrap_manifest_derives_sync_endpoint() {
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

        let parsed = parse_bootstrap_manifest(payload).expect("manifest parse");
        assert_eq!(
            parsed.sync_token_endpoint.as_deref(),
            Some("https://api.example.com/v1/sync/token")
        );
        assert_eq!(parsed.api_base_url, "https://api.example.com");
    }
}
