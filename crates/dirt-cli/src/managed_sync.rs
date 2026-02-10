//! Managed sync token exchange client for CLI profiles.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

use crate::config_profiles::is_http_url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedSyncToken {
    pub auth_token: String,
    pub database_url: String,
    pub expires_at: i64,
}

#[derive(Debug, Error)]
pub enum ManagedSyncError {
    #[error("Invalid managed sync configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("Managed sync HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Managed sync API error: {0}")]
    Api(String),
}

type ManagedSyncResult<T> = Result<T, ManagedSyncError>;

#[derive(Clone)]
pub struct ManagedSyncAuthClient {
    endpoint: String,
    client: reqwest::Client,
}

impl ManagedSyncAuthClient {
    pub fn new(endpoint: impl Into<String>) -> ManagedSyncResult<Self> {
        let endpoint = normalize_endpoint(&endpoint.into())?;
        Ok(Self {
            endpoint,
            client: reqwest::Client::builder().build()?,
        })
    }

    pub async fn exchange_token(
        &self,
        supabase_access_token: &str,
    ) -> ManagedSyncResult<ManagedSyncToken> {
        let supabase_access_token = supabase_access_token.trim();
        if supabase_access_token.is_empty() {
            return Err(ManagedSyncError::InvalidConfiguration(
                "Supabase access token must not be empty",
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
            return Err(ManagedSyncError::Api(parse_api_error(status, &body)));
        }

        let payload = response.json::<ManagedSyncTokenResponse>().await?;
        let auth_token = payload
            .auth_token
            .or(payload.token)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ManagedSyncError::Api("Response did not include a sync token".to_string())
            })?;
        let database_url = payload
            .database_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ManagedSyncError::Api("Response did not include database_url".to_string())
            })?;
        let expires_at = payload
            .expires_at
            .or_else(|| {
                payload
                    .expires_in
                    .map(|expires| unix_timestamp_now().saturating_add(expires))
            })
            .ok_or_else(|| {
                ManagedSyncError::Api("Response did not include token expiry".to_string())
            })?;

        Ok(ManagedSyncToken {
            auth_token,
            database_url,
            expires_at,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ManagedSyncTokenResponse {
    auth_token: Option<String>,
    token: Option<String>,
    database_url: Option<String>,
    expires_at: Option<i64>,
    expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ManagedSyncErrorBody {
    error: Option<String>,
    message: Option<String>,
}

fn parse_api_error(status: StatusCode, body: &str) -> String {
    if let Ok(payload) = serde_json::from_str::<ManagedSyncErrorBody>(body) {
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

fn normalize_endpoint(endpoint: &str) -> ManagedSyncResult<String> {
    let endpoint = endpoint.trim().trim_end_matches('/').to_string();
    if endpoint.is_empty() {
        return Err(ManagedSyncError::InvalidConfiguration(
            "Managed sync endpoint must not be empty",
        ));
    }
    if !is_http_url(&endpoint) {
        return Err(ManagedSyncError::InvalidConfiguration(
            "Managed sync endpoint must include http:// or https://",
        ));
    }
    Ok(endpoint)
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
        let empty = normalize_endpoint("  ").unwrap_err();
        assert!(empty.to_string().contains("must not be empty"));

        let missing_scheme = normalize_endpoint("api.example.com").unwrap_err();
        assert!(missing_scheme.to_string().contains("http:// or https://"));
    }

    #[test]
    fn normalize_endpoint_trims_trailing_slash() {
        let normalized = normalize_endpoint("https://api.example.com/v1/sync/token/").unwrap();
        assert_eq!(normalized, "https://api.example.com/v1/sync/token");
    }
}
