//! Mobile bootstrap configuration loaded from build-time generated JSON.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

pub use dirt_core::config::normalize_text_option;
use dirt_core::config::{
    resolve_bootstrap_config as resolve_core_bootstrap_config, BootstrapConfig,
};

pub type MobileBootstrapConfig = BootstrapConfig;

/// Loads the generated mobile bootstrap JSON from `OUT_DIR`.
pub fn load_bootstrap_config() -> MobileBootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/mobile-bootstrap.json"));
    serde_json::from_str(raw).unwrap_or_else(|error| {
        tracing::warn!("Failed to parse mobile bootstrap config: {}", error);
        MobileBootstrapConfig::default()
    })
}

/// Resolve runtime bootstrap config with managed manifest fallback.
pub async fn resolve_bootstrap_config(fallback: MobileBootstrapConfig) -> MobileBootstrapConfig {
    resolve_core_bootstrap_config(fallback).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_text_option_rejects_empty() {
        assert_eq!(normalize_text_option(None), None);
        assert_eq!(normalize_text_option(Some("   ".to_string())), None);
    }

    #[test]
    fn normalize_text_option_trims_value() {
        assert_eq!(
            normalize_text_option(Some(" https://example.com ".to_string())),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn managed_api_base_url_falls_back_to_sync_endpoint_prefix() {
        let config = MobileBootstrapConfig {
            turso_sync_token_endpoint: Some("https://api.example.com/v1/sync/token".to_string()),
            ..Default::default()
        };
        assert_eq!(
            config.managed_api_base_url().as_deref(),
            Some("https://api.example.com")
        );
    }
}
