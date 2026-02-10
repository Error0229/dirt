//! Mobile bootstrap configuration loaded from build-time generated JSON.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileBootstrapConfig {
    #[serde(default)]
    pub supabase_url: Option<String>,
    #[serde(default)]
    pub supabase_anon_key: Option<String>,
    #[serde(default)]
    pub turso_sync_token_endpoint: Option<String>,
}

pub fn load_bootstrap_config() -> MobileBootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/mobile-bootstrap.json"));
    serde_json::from_str(raw).unwrap_or_else(|error| {
        tracing::warn!("Failed to parse mobile bootstrap config: {}", error);
        MobileBootstrapConfig::default()
    })
}

pub fn normalize_text_option(value: Option<String>) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
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
}
