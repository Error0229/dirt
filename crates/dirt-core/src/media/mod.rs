//! Backend media signing client for attachment operations.
//!
//! Platform-agnostic HTTP client that uses backend-issued presigned URLs
//! to upload, download, and delete attachments from cloud storage.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::util::compact_text;

/// HTTP client for managed media operations backed by the Dirt API service.
#[derive(Debug, Clone)]
pub struct MediaApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl MediaApiClient {
    /// Builds a client for an explicit API base URL.
    pub fn new(base_url: impl Into<String>) -> Result<Self, String> {
        let base_url = normalize_base_url(base_url.into().as_str())?;
        let client = reqwest::Client::builder()
            .build()
            .map_err(|error| format!("Failed to construct HTTP client: {error}"))?;
        Ok(Self { base_url, client })
    }

    /// Returns the base URL this client was configured with.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Uploads attachment bytes using a backend-issued presigned operation.
    pub async fn upload(
        &self,
        access_token: &str,
        object_key: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let operation = self
            .request_presigned(
                access_token,
                "/v1/media/presign/upload",
                &serde_json::json!({
                    "object_key": object_key,
                    "content_type": content_type,
                }),
            )
            .await?;

        let method = parse_method(&operation.method)?;
        let mut request = self.client.request(method, &operation.url);
        for (name, value) in operation.headers {
            if name.eq_ignore_ascii_case("host") {
                continue;
            }
            request = request.header(name, value);
        }
        let response = request
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|error| format!("Upload request failed: {error}"))?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Upload request failed with HTTP {status}: {}",
                compact_text(&body)
            ));
        }
        Ok(())
    }

    /// Downloads attachment bytes using a backend-issued presigned operation.
    ///
    /// Returns raw bytes and an optional content type returned by the storage backend.
    pub async fn download(
        &self,
        access_token: &str,
        object_key: &str,
    ) -> Result<(Vec<u8>, Option<String>), String> {
        let encoded_object_key = urlencoding::encode(object_key);
        let url = format!(
            "{}/v1/media/presign/download?object_key={}",
            self.base_url, encoded_object_key
        );

        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| format!("Failed to request download URL: {error}"))?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Download URL request failed with HTTP {status}: {}",
                compact_text(&body)
            ));
        }
        let payload = response
            .json::<PresignResponse>()
            .await
            .map_err(|error| format!("Failed to parse download URL response: {error}"))?;

        let operation = payload.operation;
        let method = parse_method(&operation.method)?;
        let mut request = self.client.request(method, &operation.url);
        for (name, value) in operation.headers {
            if name.eq_ignore_ascii_case("host") {
                continue;
            }
            request = request.header(name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("Download request failed: {error}"))?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Download request failed with HTTP {status}: {}",
                compact_text(&body)
            ));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string);
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("Failed to read attachment bytes: {error}"))?;
        Ok((bytes.to_vec(), content_type))
    }

    /// Deletes an attachment object using a backend-issued presigned operation.
    pub async fn delete(&self, access_token: &str, object_key: &str) -> Result<(), String> {
        let operation = self
            .request_presigned(
                access_token,
                "/v1/media/presign/delete",
                &serde_json::json!({
                    "object_key": object_key,
                }),
            )
            .await?;

        let method = parse_method(&operation.method)?;
        let mut request = self.client.request(method, &operation.url);
        for (name, value) in operation.headers {
            if name.eq_ignore_ascii_case("host") {
                continue;
            }
            request = request.header(name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("Delete request failed: {error}"))?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Delete request failed with HTTP {status}: {}",
                compact_text(&body)
            ));
        }
        Ok(())
    }

    async fn request_presigned(
        &self,
        access_token: &str,
        route: &str,
        body: &serde_json::Value,
    ) -> Result<PresignedOperation, String> {
        let response = self
            .client
            .post(format!("{}{}", self.base_url, route))
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|error| format!("Failed to request signed URL: {error}"))?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Signed URL request failed with HTTP {status}: {}",
                compact_text(&body)
            ));
        }
        let payload = response
            .json::<PresignResponse>()
            .await
            .map_err(|error| format!("Failed to parse signed URL response: {error}"))?;
        Ok(payload.operation)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PresignResponse {
    operation: PresignedOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PresignedOperation {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
}

fn normalize_base_url(raw: &str) -> Result<String, String> {
    let base = raw.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("API base URL must not be empty".to_string());
    }
    if !(base.starts_with("https://") || base.starts_with("http://")) {
        return Err("API base URL must include http:// or https://".to_string());
    }
    Ok(base)
}

fn parse_method(raw: &str) -> Result<Method, String> {
    Method::from_bytes(raw.as_bytes()).map_err(|error| format!("Unsupported HTTP method: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_rejects_invalid_values() {
        assert!(normalize_base_url("").is_err());
        assert!(normalize_base_url("example.com").is_err());
    }

    #[test]
    fn normalize_base_url_trims_trailing_slash() {
        assert_eq!(
            normalize_base_url("https://api.example.com/").unwrap(),
            "https://api.example.com"
        );
    }
}
