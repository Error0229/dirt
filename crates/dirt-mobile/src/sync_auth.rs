//! Managed Turso sync token exchange client for mobile.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::ops::Deref;

use dirt_core::sync::TursoSyncAuthClient as CoreSyncAuthClient;
pub use dirt_core::sync::{SyncAuthError, SyncToken};

use crate::bootstrap_config::MobileBootstrapConfig;

type SyncAuthResult<T> = Result<T, SyncAuthError>;

#[derive(Clone)]
pub struct TursoSyncAuthClient {
    inner: CoreSyncAuthClient,
}

impl TursoSyncAuthClient {
    /// Create a token exchange client from bootstrap config.
    pub fn new_from_bootstrap(config: &MobileBootstrapConfig) -> SyncAuthResult<Option<Self>> {
        let Some(endpoint) = config.turso_sync_token_endpoint.clone() else {
            return Ok(None);
        };
        Ok(Some(Self::new(endpoint)?))
    }

    /// Create a token exchange client with explicit endpoint.
    pub fn new(endpoint: impl Into<String>) -> SyncAuthResult<Self> {
        let inner = CoreSyncAuthClient::new(endpoint.into())?;
        Ok(Self { inner })
    }

    /// Exchange a Supabase access token for short-lived Turso credentials.
    pub async fn exchange_token(&self, supabase_access_token: &str) -> SyncAuthResult<SyncToken> {
        self.inner.exchange_token(supabase_access_token).await
    }
}

impl Deref for TursoSyncAuthClient {
    type Target = CoreSyncAuthClient;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap_config::MobileBootstrapConfig;

    #[test]
    fn normalize_endpoint_rejects_empty() {
        let error = TursoSyncAuthClient::new("  ").err().unwrap();
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn normalize_endpoint_rejects_missing_scheme() {
        let error = TursoSyncAuthClient::new("example.com/token").err().unwrap();
        assert!(error.to_string().contains("http:// or https://"));
    }

    #[test]
    fn normalize_endpoint_trims_trailing_slash() {
        let client = TursoSyncAuthClient::new("https://example.com/token/").unwrap();
        assert_eq!(client.endpoint(), "https://example.com/token");
    }

    #[test]
    fn sync_token_debug_redacts_token() {
        let token = SyncToken {
            token: "sensitive-token".to_string(),
            expires_at: 1_700_000_000,
            database_url: "libsql://example.turso.io".to_string(),
        };
        let debug_output = format!("{token:?}");
        assert!(!debug_output.contains("sensitive-token"));
        assert!(debug_output.contains("[REDACTED]"));
    }

    #[test]
    fn new_from_bootstrap_returns_none_when_missing_endpoint() {
        let config = MobileBootstrapConfig::default();
        assert!(TursoSyncAuthClient::new_from_bootstrap(&config)
            .unwrap()
            .is_none());
    }
}
