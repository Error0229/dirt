//! Desktop bootstrap configuration loaded from build-time generated JSON.
//!
//! Re-exports the shared `BootstrapConfig` from dirt-core and provides
//! the desktop-specific `load_bootstrap_config` function that reads the
//! embedded build-time JSON.

pub use dirt_core::config::{resolve_bootstrap_config, BootstrapConfig};

/// Loads the generated desktop bootstrap JSON from `OUT_DIR`.
pub fn load_bootstrap_config() -> BootstrapConfig {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/desktop-bootstrap.json"));
    let parsed: BootstrapConfig = serde_json::from_str(raw)
        .unwrap_or_else(|error| panic!("Failed to parse desktop bootstrap config: {error}"));
    normalize_desktop_bootstrap(parsed)
}

fn normalize_desktop_bootstrap(mut config: BootstrapConfig) -> BootstrapConfig {
    config.bootstrap_manifest_url = normalize_desktop_url(config.bootstrap_manifest_url);
    config.turso_sync_token_endpoint = normalize_desktop_url(config.turso_sync_token_endpoint);
    config.dirt_api_base_url = normalize_desktop_url(config.dirt_api_base_url);
    config
}

fn normalize_desktop_url(value: Option<String>) -> Option<String> {
    value.map(|raw| {
        raw.trim()
            .trim_end_matches('/')
            .replace("://10.0.2.2", "://127.0.0.1")
            .replace("://localhost", "://127.0.0.1")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_desktop_bootstrap_rewrites_emulator_hosts() {
        let config = BootstrapConfig {
            bootstrap_manifest_url: Some("http://10.0.2.2:8080/v1/bootstrap".to_string()),
            turso_sync_token_endpoint: Some("http://localhost:8080/v1/sync/token".to_string()),
            dirt_api_base_url: Some("http://10.0.2.2:8080".to_string()),
            ..BootstrapConfig::default()
        };

        let normalized = normalize_desktop_bootstrap(config);
        assert_eq!(
            normalized.bootstrap_manifest_url.as_deref(),
            Some("http://127.0.0.1:8080/v1/bootstrap")
        );
        assert_eq!(
            normalized.turso_sync_token_endpoint.as_deref(),
            Some("http://127.0.0.1:8080/v1/sync/token")
        );
        assert_eq!(
            normalized.dirt_api_base_url.as_deref(),
            Some("http://127.0.0.1:8080")
        );
    }
}
