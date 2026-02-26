//! Shared managed Turso sync token exchange client.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

use crate::config::normalize_text_option;

#[derive(Clone, PartialEq, Eq)]
pub struct SyncToken {
    pub token: String,
    pub expires_at: i64,
    pub database_url: Option<String>,
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

#[derive(Debug, Error)]
pub enum SyncAuthError {
    #[error("Invalid sync auth configuration: {0}")]
    InvalidConfiguration(String),
    #[error("Sync auth HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Sync auth API error: {0}")]
    Api(String),
    #[error("Invalid sync token payload: {0}")]
    InvalidPayload(String),
}

pub type SyncAuthResult<T> = Result<T, SyncAuthError>;

#[derive(Clone)]
pub struct TursoSyncAuthClient {
    endpoint: String,
    client: reqwest::Client,
}

impl TursoSyncAuthClient {
    pub fn new(endpoint: impl Into<String>) -> SyncAuthResult<Self> {
        let endpoint = normalize_endpoint(endpoint.into())?;
        Ok(Self {
            endpoint,
            client: reqwest::Client::builder().build()?,
        })
    }

    pub async fn exchange_token(&self, supabase_access_token: &str) -> SyncAuthResult<SyncToken> {
        let supabase_access_token = supabase_access_token.trim();
        if supabase_access_token.is_empty() {
            return Err(SyncAuthError::InvalidConfiguration(
                "Supabase access token must not be empty".to_string(),
            ));
        }

        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(supabase_access_token)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SyncAuthError::Api(parse_api_error(status, &body)));
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
    expires_in: Option<i64>,
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

        let expires_at = value
            .expires_at
            .or_else(|| {
                value
                    .expires_in
                    .map(|expires_in| unix_timestamp_now().saturating_add(expires_in))
            })
            .ok_or_else(|| {
                SyncAuthError::InvalidPayload(
                    "response did not include expires_at/expires_in".to_string(),
                )
            })?;

        let database_url = value
            .database_url
            .map(|database_url| database_url.trim().to_string())
            .filter(|database_url| !database_url.is_empty());

        Ok(Self {
            token,
            expires_at,
            database_url,
        })
    }
}

#[derive(Debug, Deserialize)]
struct SyncAuthErrorBody {
    error: Option<String>,
    message: Option<String>,
}

fn parse_api_error(status: StatusCode, body: &str) -> String {
    if let Ok(payload) = serde_json::from_str::<SyncAuthErrorBody>(body) {
        if let Some(message) = payload.message.or(payload.error) {
            return format!("{} ({})", message.trim(), status.as_u16());
        }
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        format!("HTTP {}", status.as_u16())
    } else {
        format!("{} ({})", trimmed, status.as_u16())
    }
}

fn normalize_endpoint(raw: String) -> SyncAuthResult<String> {
    let endpoint = normalize_text_option(Some(raw)).ok_or_else(|| {
        SyncAuthError::InvalidConfiguration("endpoint must not be empty".to_string())
    })?;
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        Ok(endpoint.trim_end_matches('/').to_string())
    } else {
        Err(SyncAuthError::InvalidConfiguration(
            "endpoint must include http:// or https://".to_string(),
        ))
    }
}

fn unix_timestamp_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
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
            database_url: Some("libsql://example.turso.io".to_string()),
        };
        let debug = format!("{token:?}");
        assert!(!debug.contains("secret"));
        assert!(debug.contains("[REDACTED]"));
    }
}
