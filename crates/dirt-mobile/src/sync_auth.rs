//! Managed Turso sync token exchange client for mobile.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use crate::bootstrap_config::MobileBootstrapConfig;

pub type SyncAuthError = dirt_core::sync::SyncAuthError;
pub type SyncToken = dirt_core::sync::SyncToken;

#[derive(Clone)]
pub struct TursoSyncAuthClient {
    inner: dirt_core::sync::TursoSyncAuthClient,
}

impl TursoSyncAuthClient {
    pub fn new_from_bootstrap(
        config: &MobileBootstrapConfig,
    ) -> Result<Option<Self>, SyncAuthError> {
        let Some(endpoint) = config.turso_sync_token_endpoint.clone() else {
            return Ok(None);
        };
        Ok(Some(Self::new(endpoint)?))
    }

    pub fn new_from_env() -> Result<Option<Self>, SyncAuthError> {
        let Some(endpoint) = std::env::var("TURSO_SYNC_TOKEN_ENDPOINT").ok() else {
            return Ok(None);
        };
        Ok(Some(Self::new(endpoint)?))
    }

    pub fn new(endpoint: impl Into<String>) -> Result<Self, SyncAuthError> {
        Ok(Self {
            inner: dirt_core::sync::TursoSyncAuthClient::new(endpoint)?,
        })
    }

    pub async fn exchange_token(
        &self,
        supabase_access_token: &str,
    ) -> Result<SyncToken, SyncAuthError> {
        self.inner.exchange_token(supabase_access_token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_from_bootstrap_returns_none_when_missing_endpoint() {
        let config = MobileBootstrapConfig::default();
        assert!(TursoSyncAuthClient::new_from_bootstrap(&config)
            .unwrap()
            .is_none());
    }

    #[test]
    fn sync_token_debug_redacts_token() {
        let token = SyncToken {
            token: "sensitive-token".to_string(),
            expires_at: 1_700_000_000,
            database_url: Some("libsql://db.turso.io".to_string()),
        };
        let debug_output = format!("{token:?}");
        assert!(!debug_output.contains("sensitive-token"));
        assert!(debug_output.contains("[REDACTED]"));
    }
}
