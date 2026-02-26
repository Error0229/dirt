//! Backend media signing client for desktop attachment operations.

use crate::bootstrap_config::DesktopBootstrapConfig;

#[derive(Debug, Clone)]
pub struct MediaApiClient {
    inner: dirt_core::media::MediaApiClient,
}

impl MediaApiClient {
    /// Builds a client from desktop bootstrap configuration.
    ///
    /// Returns `Ok(None)` when managed media is not configured.
    pub fn new_from_bootstrap(config: &DesktopBootstrapConfig) -> Result<Option<Self>, String> {
        let Some(base_url) = config.managed_api_base_url() else {
            return Ok(None);
        };
        Ok(Some(Self::new(base_url)?))
    }

    /// Builds a client for an explicit API base URL.
    pub fn new(base_url: impl Into<String>) -> Result<Self, String> {
        Ok(Self {
            inner: dirt_core::media::MediaApiClient::new(base_url)?,
        })
    }

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

    pub async fn download(
        &self,
        access_token: &str,
        object_key: &str,
    ) -> Result<(Vec<u8>, Option<String>), String> {
        self.inner.download(access_token, object_key).await
    }

    pub async fn delete(&self, access_token: &str, object_key: &str) -> Result<(), String> {
        self.inner.delete(access_token, object_key).await
    }
}
