//! Managed bootstrap manifest client for CLI profile initialization.

use thiserror::Error;

pub type ManagedBootstrapConfig = dirt_core::config::ManagedBootstrapConfig;

#[derive(Debug, Error)]
pub enum ManagedBootstrapError {
    #[error("{0}")]
    Message(String),
}

impl From<String> for ManagedBootstrapError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

pub async fn fetch_bootstrap_manifest(
    bootstrap_url: &str,
) -> Result<ManagedBootstrapConfig, ManagedBootstrapError> {
    dirt_core::config::fetch_managed_bootstrap_manifest(bootstrap_url)
        .await
        .map_err(Into::into)
}

#[allow(dead_code)]
pub fn parse_bootstrap_manifest(
    payload: &str,
) -> Result<ManagedBootstrapConfig, ManagedBootstrapError> {
    dirt_core::config::parse_managed_bootstrap_manifest(payload).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    async fn spawn_one_shot_server(status_line: &str, body: &str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("local address");
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );

        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut request_buffer = [0_u8; 1024];
                let _ = socket.read(&mut request_buffer).await;
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        format!("http://{address}/v1/bootstrap")
    }

    #[test]
    fn parse_bootstrap_manifest_rejects_unknown_fields() {
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
            "unknown": true
          }
        }
        "#;

        let error = parse_bootstrap_manifest(payload).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn parse_bootstrap_manifest_derives_sync_endpoint() {
        let payload = r#"
        {
          "schema_version": 1,
          "manifest_version": "v1",
          "supabase_url": "https://project.supabase.co",
          "supabase_anon_key": "anon",
          "api_base_url": "https://api.example.com",
          "feature_flags": {
            "managed_sync": true,
            "managed_media": false
          }
        }
        "#;

        let parsed = parse_bootstrap_manifest(payload).expect("manifest parse");
        assert_eq!(
            parsed.sync_token_endpoint.as_deref(),
            Some("https://api.example.com/v1/sync/token")
        );
        assert_eq!(parsed.api_base_url, "https://api.example.com");
    }

    #[tokio::test]
    async fn fetch_bootstrap_manifest_parses_valid_payload() {
        let body = r#"
        {
          "schema_version": 1,
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
        let url = spawn_one_shot_server("200 OK", body).await;

        let parsed = fetch_bootstrap_manifest(&url)
            .await
            .expect("bootstrap fetch should succeed");
        assert_eq!(
            parsed.sync_token_endpoint.as_deref(),
            Some("https://api.example.com/v1/sync/token")
        );
        assert!(parsed.managed_sync);
        assert!(parsed.managed_media);
    }

    #[tokio::test]
    async fn fetch_bootstrap_manifest_surfaces_http_failure() {
        let url = spawn_one_shot_server("500 Internal Server Error", "{\"error\":\"boom\"}").await;
        let error = fetch_bootstrap_manifest(&url)
            .await
            .expect_err("bootstrap fetch should fail");
        assert!(error.to_string().contains("HTTP 500"));
    }
}
