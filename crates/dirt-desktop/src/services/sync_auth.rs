//! Managed Turso sync token exchange client for desktop.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bootstrap_config::{normalize_text_option, DesktopBootstrapConfig};

/// Short-lived Turso sync credentials minted by backend auth exchange.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncToken {
    /// Token to use as `TURSO_AUTH_TOKEN`.
    pub token: String,
    /// Unix timestamp (seconds) when token expires.
    pub expires_at: i64,
    /// Turso database URL to pair with the token.
    pub database_url: String,
}

impl std::fmt::Debug for SyncToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyncToken")
            .field("token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("database_url", &self.database_url)
            .finish()
    }
}

/// Errors returned by managed sync auth client.
#[derive(Debug, Error)]
pub enum SyncAuthError {
    #[error("Invalid sync auth configuration: {0}")]
    InvalidConfiguration(String),
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Sync API error: {0}")]
    Api(String),
    #[error("Invalid sync token payload: {0}")]
    InvalidPayload(String),
}

type SyncAuthResult<T> = Result<T, SyncAuthError>;

/// Backend token exchange client.
#[derive(Clone)]
pub struct TursoSyncAuthClient {
    endpoint: String,
    client: Client,
}

impl TursoSyncAuthClient {
    /// Creates a client from desktop bootstrap configuration.
    pub fn new_from_bootstrap(config: &DesktopBootstrapConfig) -> SyncAuthResult<Option<Self>> {
        let Some(endpoint) = config.turso_sync_token_endpoint.clone() else {
            return Ok(None);
        };
        Ok(Some(Self::new(endpoint)?))
    }

    /// Creates a client with explicit endpoint URL.
    pub fn new(endpoint: impl Into<String>) -> SyncAuthResult<Self> {
        let endpoint = normalize_endpoint(endpoint.into())?;
        Ok(Self {
            endpoint,
            client: Client::new(),
        })
    }

    /// Exchanges Supabase access token for short-lived Turso credentials.
    pub async fn exchange_token(&self, supabase_access_token: &str) -> SyncAuthResult<SyncToken> {
        let access_token = supabase_access_token.trim();
        if access_token.is_empty() {
            return Err(SyncAuthError::InvalidConfiguration(
                "Supabase access token must not be empty".to_string(),
            ));
        }

        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(SyncAuthError::Api(format!(
                "HTTP {status}: {}",
                compact_body(&body)
            )));
        }

        let payload = response.json::<SyncTokenResponse>().await?;
        payload.try_into()
    }
}

#[derive(Debug, Deserialize)]
struct SyncTokenResponse {
    auth_token: Option<String>,
    token: Option<String>,
    expires_at: Option<i64>,
    database_url: Option<String>,
}

impl TryFrom<SyncTokenResponse> for SyncToken {
    type Error = SyncAuthError;

    fn try_from(value: SyncTokenResponse) -> SyncAuthResult<Self> {
        let token = value
            .auth_token
            .or(value.token)
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                SyncAuthError::InvalidPayload(
                    "response did not include auth_token/token".to_string(),
                )
            })?;

        let expires_at = value.expires_at.ok_or_else(|| {
            SyncAuthError::InvalidPayload("response did not include expires_at".to_string())
        })?;

        let database_url = normalize_text_option(value.database_url).ok_or_else(|| {
            SyncAuthError::InvalidPayload("response did not include database_url".to_string())
        })?;

        Ok(Self {
            token,
            expires_at,
            database_url,
        })
    }
}

fn normalize_endpoint(raw: String) -> SyncAuthResult<String> {
    let normalized = normalize_text_option(Some(raw)).ok_or_else(|| {
        SyncAuthError::InvalidConfiguration("endpoint must not be empty".to_string())
    })?;
    if normalized.starts_with("http://") || normalized.starts_with("https://") {
        Ok(normalized.trim_end_matches('/').to_string())
    } else {
        Err(SyncAuthError::InvalidConfiguration(
            "endpoint must include http:// or https://".to_string(),
        ))
    }
}

fn compact_body(body: &str) -> String {
    body.trim().chars().take(180).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_endpoint_rejects_invalid_values() {
        assert!(normalize_endpoint(String::new()).is_err());
        assert!(normalize_endpoint("api.example.com".to_string()).is_err());
    }

    #[test]
    fn sync_token_debug_redacts_token() {
        let token = SyncToken {
            token: "secret".to_string(),
            expires_at: 123,
            database_url: "libsql://example.turso.io".to_string(),
        };
        let debug = format!("{token:?}");
        assert!(!debug.contains("secret"));
        assert!(debug.contains("[REDACTED]"));
    }
}
