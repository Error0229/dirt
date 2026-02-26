//! Backend media signing client for mobile attachment operations.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use crate::bootstrap_config::MobileBootstrapConfig;

#[derive(Debug, Clone)]
pub struct MediaApiClient {
    inner: dirt_core::media::MediaApiClient,
}

impl MediaApiClient {
    pub fn new_from_bootstrap(config: &MobileBootstrapConfig) -> Result<Option<Self>, String> {
        let Some(base_url) = config.managed_api_base_url() else {
            return Ok(None);
        };
        Ok(Some(Self::new(base_url)?))
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_rejects_invalid_values() {
        let err = MediaApiClient::new("   ").unwrap_err();
        assert!(err.contains("must not be empty"));

        let err = MediaApiClient::new("api.example.com").unwrap_err();
        assert!(err.contains("http:// or https://"));
    }

    #[test]
    fn normalize_base_url_trims_trailing_slash() {
        assert!(MediaApiClient::new("https://api.example.com/").is_ok());
    }
}
