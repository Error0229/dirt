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

    #[test]
    fn parse_manifest_rejects_unknown_fields() {
        let payload = r#"
        {
          "schema_version": 1,
          "manifest_version": "v1",
          "supabase_url": "https://project.supabase.co",
          "supabase_anon_key": "anon",
          "api_base_url": "https://api.example.com",
          "feature_flags": {
            "managed_sync": true,
            "managed_media": true,
            "unexpected": true
          }
        }
        "#;

        let error = dirt_core::config::parse_bootstrap_manifest(
            payload,
            "https://api.example.com/v1/bootstrap",
        )
        .unwrap_err();
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn parse_manifest_rejects_invalid_schema_version() {
        let payload = r#"
        {
          "schema_version": 2,
          "manifest_version": "v1",
          "supabase_url": "https://project.supabase.co",
          "supabase_anon_key": "anon",
          "api_base_url": "https://api.example.com",
          "feature_flags": {
            "managed_sync": true,
            "managed_media": true
          }
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
    fn parse_manifest_derives_sync_endpoint_when_missing() {
        let payload = r#"
        {
          "schema_version": 1,
          "manifest_version": "v2",
          "supabase_url": "https://project.supabase.co",
          "supabase_anon_key": "anon",
          "api_base_url": "https://api.example.com",
          "feature_flags": {
            "managed_sync": true,
            "managed_media": false
          }
        }
        "#;

        let parsed = dirt_core::config::parse_bootstrap_manifest(
            payload,
            "https://api.example.com/v1/bootstrap",
        )
        .expect("manifest should parse");
        assert_eq!(
            parsed.turso_sync_token_endpoint.as_deref(),
            Some("https://api.example.com/v1/sync/token")
        );
        assert_eq!(
            parsed.dirt_api_base_url.as_deref(),
            Some("https://api.example.com")
        );
    }
}
