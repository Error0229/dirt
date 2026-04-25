//! Mobile bootstrap configuration loaded from generated JSON.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

pub use dirt_core::config::BootstrapConfig as MobileBootstrapConfig;
pub use dirt_core::util::normalize_text_option;

/// Loads the generated mobile bootstrap JSON from `OUT_DIR`.
pub fn load_bootstrap_config() -> MobileBootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/mobile-bootstrap.json"));
    serde_json::from_str(raw)
        .unwrap_or_else(|error| panic!("Failed to parse mobile bootstrap config: {error}"))
}

/// Resolves runtime bootstrap config.
pub async fn resolve_bootstrap_config(
    fallback: MobileBootstrapConfig,
) -> Result<MobileBootstrapConfig, String> {
    dirt_core::config::resolve_bootstrap_config(fallback).await
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
    fn managed_api_base_url_returns_configured_value() {
        let config = MobileBootstrapConfig {
            dirt_api_base_url: Some("https://api.example.com".to_string()),
            ..Default::default()
        };
        assert_eq!(
            config.managed_api_base_url().as_deref(),
            Some("https://api.example.com")
        );
    }

    #[test]
    fn parse_manifest_rejects_invalid_schema_version() {
        let payload = r#"
        {
          "schema_version": 9,
          "manifest_version": "v1",
          "api_base_url": "https://api.example.com"
        }
        "#;

        let error = dirt_core::config::parse_bootstrap_manifest(
            payload,
            "https://api.example.com/v1/bootstrap",
        )
        .unwrap_err();
        assert!(error.contains("schema_version"));
    }

    #[test]
    fn parse_manifest_returns_api_base_url() {
        let payload = r#"
        {
          "schema_version": 2,
          "manifest_version": "v2",
          "api_base_url": "https://api.example.com"
        }
        "#;

        let parsed = dirt_core::config::parse_bootstrap_manifest(
            payload,
            "https://api.example.com/v1/bootstrap",
        )
        .expect("manifest should parse");
        assert_eq!(
            parsed.dirt_api_base_url.as_deref(),
            Some("https://api.example.com")
        );
    }
}
