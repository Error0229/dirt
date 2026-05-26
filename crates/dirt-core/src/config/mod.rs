//! Bootstrap configuration for client apps.
//!
//! `BootstrapConfig` carries the build-baked API base URL clients need
//! at startup. Phase-1's runtime "bootstrap manifest" (`/v1/bootstrap`
//! endpoint, [`ManagedBootstrapManifest`], runtime fetch) was deleted
//! when the server-side route was removed; clients now read the
//! embedded JSON straight from `build.rs` and never hit the network at
//! startup. Bearer-token secrets live in the OS keychain (clients)
//! or in `DIRT_SERVER_TOKEN` (server), never here.

use serde::{Deserialize, Serialize};

use crate::util::normalize_text_option;

/// Build-provisioned client configuration.
///
/// Safe-to-ship public endpoints required to bootstrap the API client.
/// Secret credentials must never be stored here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BootstrapConfig {
    #[serde(default)]
    pub dirt_api_base_url: Option<String>,
}

impl BootstrapConfig {
    /// Returns the dirt-api base URL when configured.
    pub fn managed_api_base_url(&self) -> Option<String> {
        normalize_text_option(self.dirt_api_base_url.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_api_base_url_returns_configured_value() {
        let config = BootstrapConfig {
            dirt_api_base_url: Some("https://api.example.com".to_string()),
        };
        assert_eq!(
            config.managed_api_base_url().as_deref(),
            Some("https://api.example.com")
        );
    }

    #[test]
    fn managed_api_base_url_normalizes_empty() {
        let config = BootstrapConfig {
            dirt_api_base_url: Some("   ".to_string()),
        };
        assert!(config.managed_api_base_url().is_none());
    }
}
