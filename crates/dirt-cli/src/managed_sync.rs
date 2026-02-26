//! Managed sync token exchange client for CLI profiles.

pub type ManagedSyncError = dirt_core::sync::SyncAuthError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedSyncToken {
    pub auth_token: String,
    pub database_url: String,
    pub expires_at: i64,
}

#[derive(Clone)]
pub struct ManagedSyncAuthClient {
    inner: dirt_core::sync::TursoSyncAuthClient,
}

impl ManagedSyncAuthClient {
    pub fn new(endpoint: impl Into<String>) -> Result<Self, ManagedSyncError> {
        Ok(Self {
            inner: dirt_core::sync::TursoSyncAuthClient::new(endpoint)?,
        })
    }

    pub async fn exchange_token(
        &self,
        supabase_access_token: &str,
    ) -> Result<ManagedSyncToken, ManagedSyncError> {
        let token = self.inner.exchange_token(supabase_access_token).await?;
        let database_url = token.database_url.ok_or_else(|| {
            ManagedSyncError::InvalidPayload("Response did not include database_url".to_string())
        })?;

        Ok(ManagedSyncToken {
            auth_token: token.token,
            database_url,
            expires_at: token.expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_endpoint_rejects_invalid_values() {
        let empty = ManagedSyncAuthClient::new("  ").err().unwrap();
        assert!(empty.to_string().contains("must not be empty"));

        let missing_scheme = ManagedSyncAuthClient::new("api.example.com").err().unwrap();
        assert!(missing_scheme.to_string().contains("http:// or https://"));
    }

    #[test]
    fn normalize_endpoint_trims_trailing_slash() {
        assert!(ManagedSyncAuthClient::new("https://api.example.com/v1/sync/token/").is_ok());
    }
}
