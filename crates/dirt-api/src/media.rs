use std::sync::Arc;
use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::Client;
use aws_types::region::Region;
use serde::Serialize;

use crate::config::{AppConfig, R2RuntimeConfig};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct PresignedOperation {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Clone)]
pub struct R2PresignService {
    bucket: String,
    ttl: Duration,
    client: Client,
}

impl R2PresignService {
    pub fn from_config(config: &Arc<AppConfig>) -> Option<Self> {
        config
            .r2
            .clone()
            .map(|r2| Self::new(r2, config.media_url_ttl))
    }

    pub fn new(config: R2RuntimeConfig, ttl: Duration) -> Self {
        let credentials = Credentials::new(
            config.access_key_id,
            config.secret_access_key,
            None,
            None,
            "dirt-api-r2",
        );

        let endpoint = format!("https://{}.r2.cloudflarestorage.com", config.account_id);
        let shared_config = aws_sdk_s3::Config::builder()
            .region(Region::new("auto"))
            .endpoint_url(endpoint)
            .credentials_provider(credentials)
            .force_path_style(true)
            .build();

        let client = Client::from_conf(shared_config);

        Self {
            bucket: config.bucket,
            ttl,
            client,
        }
    }

    pub async fn presign_upload(
        &self,
        object_key: &str,
        content_type: Option<&str>,
    ) -> Result<PresignedOperation, AppError> {
        let object_key = normalize_object_key(object_key)?;
        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(&object_key);
        if let Some(content_type) = content_type.and_then(normalize_content_type) {
            request = request.content_type(content_type);
        }
        let operation = request
            .presigned(presign_config(self.ttl)?)
            .await
            .map_err(|error| {
                AppError::external(format!(
                    "Failed to presign upload URL: {}",
                    sanitize(&error)
                ))
            })?;
        Ok(map_presigned(
            operation.method().to_string(),
            operation.uri().to_string(),
            operation.headers(),
        ))
    }

    pub async fn presign_download(&self, object_key: &str) -> Result<PresignedOperation, AppError> {
        let object_key = normalize_object_key(object_key)?;
        let operation = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&object_key)
            .presigned(presign_config(self.ttl)?)
            .await
            .map_err(|error| {
                AppError::external(format!(
                    "Failed to presign download URL: {}",
                    sanitize(&error)
                ))
            })?;
        Ok(map_presigned(
            operation.method().to_string(),
            operation.uri().to_string(),
            operation.headers(),
        ))
    }

    pub async fn presign_delete(&self, object_key: &str) -> Result<PresignedOperation, AppError> {
        let object_key = normalize_object_key(object_key)?;
        let operation = self
            .client
            .delete_object()
            .bucket(&self.bucket)
            .key(&object_key)
            .presigned(presign_config(self.ttl)?)
            .await
            .map_err(|error| {
                AppError::external(format!(
                    "Failed to presign delete URL: {}",
                    sanitize(&error)
                ))
            })?;
        Ok(map_presigned(
            operation.method().to_string(),
            operation.uri().to_string(),
            operation.headers(),
        ))
    }
}

fn normalize_content_type(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn map_presigned<'a>(
    method: String,
    url: String,
    headers: impl Iterator<Item = (&'a str, &'a str)>,
) -> PresignedOperation {
    let headers = headers
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    PresignedOperation {
        method,
        url,
        headers,
    }
}

fn normalize_object_key(raw: &str) -> Result<String, AppError> {
    let key = raw.trim().trim_start_matches('/').to_string();
    if key.is_empty() {
        return Err(AppError::bad_request("object_key is required"));
    }
    if key.contains("..") {
        return Err(AppError::bad_request(
            "object_key must not contain path traversal segments",
        ));
    }
    Ok(key)
}

fn presign_config(ttl: Duration) -> Result<PresigningConfig, AppError> {
    PresigningConfig::expires_in(ttl)
        .map_err(|error| AppError::internal(format!("Invalid presign TTL: {}", sanitize(&error))))
}

fn sanitize(error: &impl std::fmt::Display) -> String {
    error.to_string().replace('\n', " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_object_key_rejects_empty_or_parent_segments() {
        assert!(normalize_object_key(" ").is_err());
        assert!(normalize_object_key("../a").is_err());
    }

    #[test]
    fn normalize_object_key_trims_prefix_slash() {
        assert_eq!(
            normalize_object_key("/notes/file.png").unwrap(),
            "notes/file.png"
        );
    }
}
