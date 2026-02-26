//! Managed Turso sync token exchange client for desktop.

use crate::bootstrap_config::{normalize_text_option, DesktopBootstrapConfig};

pub type SyncAuthError = dirt_core::sync::SyncAuthError;

#[derive(Clone, PartialEq, Eq)]
pub struct SyncToken {
    pub token: String,
    pub expires_at: i64,
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

#[derive(Clone)]
pub struct TursoSyncAuthClient {
    inner: dirt_core::sync::TursoSyncAuthClient,
}

impl TursoSyncAuthClient {
    pub fn new_from_bootstrap(
        config: &DesktopBootstrapConfig,
    ) -> Result<Option<Self>, SyncAuthError> {
        let Some(endpoint) = config.turso_sync_token_endpoint.clone() else {
            return Ok(None);
        };
        Ok(Some(Self::new(endpoint)?))
    }

    pub fn new(endpoint: impl Into<String>) -> Result<Self, SyncAuthError> {
        let endpoint = normalize_text_option(Some(endpoint.into())).ok_or_else(|| {
            SyncAuthError::InvalidConfiguration("endpoint must not be empty".to_string())
        })?;
        Ok(Self {
            inner: dirt_core::sync::TursoSyncAuthClient::new(endpoint)?,
        })
    }

    pub async fn exchange_token(
        &self,
        supabase_access_token: &str,
    ) -> Result<SyncToken, SyncAuthError> {
        let token = self.inner.exchange_token(supabase_access_token).await?;
        let database_url = normalize_text_option(token.database_url).ok_or_else(|| {
            SyncAuthError::InvalidPayload("response did not include database_url".to_string())
        })?;

        Ok(SyncToken {
            token: token.token,
            expires_at: token.expires_at,
            database_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
