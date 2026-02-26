//! Backend media signing client for mobile attachment operations.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::ops::Deref;

use dirt_core::media::MediaApiClient as CoreMediaApiClient;

use crate::bootstrap_config::MobileBootstrapConfig;

/// HTTP client for managed media operations backed by the Dirt API service.
#[derive(Debug, Clone)]
pub struct MediaApiClient {
    inner: CoreMediaApiClient,
}

impl MediaApiClient {
    /// Builds a client from mobile bootstrap configuration.
    ///
    /// Returns `Ok(None)` when managed media is not configured.
    pub fn new_from_bootstrap(config: &MobileBootstrapConfig) -> Result<Option<Self>, String> {
        let Some(base_url) = config.managed_api_base_url() else {
            return Ok(None);
        };
        Ok(Some(Self::new(base_url)?))
    }

    /// Builds a client for an explicit API base URL.
    pub fn new(base_url: impl Into<String>) -> Result<Self, String> {
        let inner = CoreMediaApiClient::new(base_url.into())?;
        Ok(Self { inner })
    }

    /// Returns the normalized API base URL used by this client.
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Uploads attachment bytes using a backend-issued presigned operation.
    pub async fn upload(
        &self,
        access_token: &str,
        object_key: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        self.inner
            .upload(access_token, object_key, content_type, bytes)
            .await
    }

    /// Downloads attachment bytes using a backend-issued presigned operation.
    pub async fn download(
        &self,
        access_token: &str,
        object_key: &str,
    ) -> Result<(Vec<u8>, Option<String>), String> {
        self.inner.download(access_token, object_key).await
    }

    /// Deletes an attachment object using a backend-issued presigned operation.
    pub async fn delete(&self, access_token: &str, object_key: &str) -> Result<(), String> {
        self.inner.delete(access_token, object_key).await
    }
}

impl Deref for MediaApiClient {
    type Target = CoreMediaApiClient;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_rejects_invalid_values() {
        assert!(MediaApiClient::new("").is_err());
        assert!(MediaApiClient::new("example.com").is_err());
    }

    #[test]
    fn normalize_base_url_trims_trailing_slash() {
        assert_eq!(
            MediaApiClient::new("https://api.example.com/")
                .unwrap()
                .base_url(),
            "https://api.example.com"
        );
    }
}
