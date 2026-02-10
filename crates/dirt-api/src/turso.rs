use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct TursoTokenBroker {
    client: reqwest::Client,
    config: Arc<AppConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MintedSyncToken {
    pub auth_token: String,
    pub expires_at: i64,
    pub database_url: String,
}

impl TursoTokenBroker {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub async fn mint_sync_token(&self, user_id: &str) -> Result<MintedSyncToken, AppError> {
        let request_url = format!(
            "{}/v1/organizations/{}/databases/{}/auth/tokens?expiration={}",
            self.config.turso_api_url.trim_end_matches('/'),
            self.config.turso_organization_slug,
            self.config.turso_database_name,
            expiration_query(self.config.turso_token_ttl),
        );

        let body = serde_json::json!({
            "permissions": {
                "full_access": true
            },
            "metadata": {
                "subject": user_id
            }
        });

        let response = self
            .client
            .post(&request_url)
            .bearer_auth(&self.config.turso_platform_api_token)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                AppError::external(format!("Turso token request failed: {}", sanitize(&error)))
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::external(format!(
                "Turso token request failed with HTTP {}: {}",
                status,
                compact_body(&body)
            )));
        }

        let payload = response
            .json::<TursoTokenResponse>()
            .await
            .map_err(|error| {
                AppError::external(format!("Turso token parse failed: {}", sanitize(&error)))
            })?;

        let token = payload
            .auth_token
            .or(payload.token)
            .or(payload.jwt)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::external("Turso token API returned no token"))?;

        let expires_at = payload.expires_at.unwrap_or_else(|| {
            Utc::now()
                .timestamp()
                .saturating_add(i64::try_from(self.config.turso_token_ttl.as_secs()).unwrap_or(900))
        });

        Ok(MintedSyncToken {
            auth_token: token,
            expires_at,
            database_url: self.config.turso_database_url.clone(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct TursoTokenResponse {
    auth_token: Option<String>,
    token: Option<String>,
    jwt: Option<String>,
    expires_at: Option<i64>,
}

fn expiration_query(ttl: Duration) -> String {
    format!("{}s", ttl.as_secs())
}

fn sanitize(error: &impl std::fmt::Display) -> String {
    error.to_string().replace('\n', " ").trim().to_string()
}

fn compact_body(body: &str) -> String {
    body.trim().chars().take(180).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiration_query_is_seconds_suffix() {
        assert_eq!(expiration_query(Duration::from_secs(900)), "900s");
    }
}
