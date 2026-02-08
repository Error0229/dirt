//! Cloudflare R2 storage configuration and key-building helpers.

use std::env;

use aws_credential_types::Credentials;
use aws_sdk_s3::{primitives::ByteStream, Client};
use aws_types::region::Region;
use chrono::Utc;
use uuid::Uuid;

use crate::{Error, Result};

const ENV_ACCOUNT_ID: &str = "R2_ACCOUNT_ID";
const ENV_BUCKET: &str = "R2_BUCKET";
const ENV_ACCESS_KEY_ID: &str = "R2_ACCESS_KEY_ID";
const ENV_SECRET_ACCESS_KEY: &str = "R2_SECRET_ACCESS_KEY";
const ENV_PUBLIC_BASE_URL: &str = "R2_PUBLIC_BASE_URL";

/// Media storage operations shared across object backends.
pub trait MediaStorage {
    /// Build a deterministic object key namespace for a note attachment.
    fn build_media_key(&self, note_id: &str, file_name: &str) -> Result<String>;

    /// Resolve a public URL for an object key when a public base URL is configured.
    fn public_object_url(&self, object_key: &str) -> Option<String>;
}

/// Cloudflare R2 configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct R2Config {
    /// Cloudflare account identifier.
    pub account_id: String,
    /// R2 bucket name.
    pub bucket: String,
    /// Access key id for S3-compatible auth.
    pub access_key_id: String,
    /// Secret access key for S3-compatible auth.
    pub secret_access_key: String,
    /// Optional public URL base for serving media.
    pub public_base_url: Option<String>,
}

impl R2Config {
    /// Load R2 configuration from environment variables.
    ///
    /// Returns `Ok(None)` when no R2 variables are set.
    /// Returns an error when only a partial configuration is provided.
    pub fn from_env() -> Result<Option<Self>> {
        parse_config(|key| env::var(key).ok())
    }

    /// Cloudflare R2 S3-compatible endpoint URL.
    #[must_use]
    pub fn endpoint_url(&self) -> String {
        format!("https://{}.r2.cloudflarestorage.com", self.account_id)
    }
}

/// R2-backed storage helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct R2Storage {
    config: R2Config,
}

impl R2Storage {
    #[must_use]
    pub const fn new(config: R2Config) -> Self {
        Self { config }
    }

    #[must_use]
    pub const fn config(&self) -> &R2Config {
        &self.config
    }

    /// Check that the configured bucket is reachable with current credentials.
    pub async fn bucket_is_reachable(&self) -> Result<()> {
        let client = self.s3_client();
        client
            .head_bucket()
            .bucket(&self.config.bucket)
            .send()
            .await
            .map_err(|error| storage_error("head_bucket", &self.config.bucket, None, error))?;
        Ok(())
    }

    /// Upload object bytes to the configured bucket.
    pub async fn upload_bytes(
        &self,
        object_key: &str,
        bytes: &[u8],
        content_type: Option<&str>,
    ) -> Result<()> {
        let object_key = normalize_object_key(object_key)?;
        let client = self.s3_client();

        let mut request = client
            .put_object()
            .bucket(&self.config.bucket)
            .key(&object_key)
            .body(ByteStream::from(bytes.to_vec()));

        if let Some(content_type) = normalize_content_type(content_type) {
            request = request.content_type(content_type);
        }

        request.send().await.map_err(|error| {
            storage_error("put_object", &self.config.bucket, Some(&object_key), error)
        })?;

        Ok(())
    }

    /// Delete an object from the configured bucket.
    pub async fn delete_object(&self, object_key: &str) -> Result<()> {
        let object_key = normalize_object_key(object_key)?;
        let client = self.s3_client();

        client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(&object_key)
            .send()
            .await
            .map_err(|error| {
                storage_error(
                    "delete_object",
                    &self.config.bucket,
                    Some(&object_key),
                    error,
                )
            })?;

        Ok(())
    }

    /// Check whether an object exists in the configured bucket.
    pub async fn object_exists(&self, object_key: &str) -> Result<bool> {
        let object_key = normalize_object_key(object_key)?;
        let client = self.s3_client();

        let response = client
            .list_objects_v2()
            .bucket(&self.config.bucket)
            .prefix(&object_key)
            .max_keys(1)
            .send()
            .await
            .map_err(|error| {
                storage_error(
                    "list_objects_v2",
                    &self.config.bucket,
                    Some(&object_key),
                    error,
                )
            })?;

        Ok(response
            .contents()
            .iter()
            .filter_map(|object| object.key())
            .any(|candidate| candidate == object_key))
    }

    /// Download object bytes from the configured bucket.
    pub async fn download_bytes(&self, object_key: &str) -> Result<(Vec<u8>, Option<String>)> {
        let object_key = normalize_object_key(object_key)?;
        let client = self.s3_client();

        let response = client
            .get_object()
            .bucket(&self.config.bucket)
            .key(&object_key)
            .send()
            .await
            .map_err(|error| {
                storage_error("get_object", &self.config.bucket, Some(&object_key), error)
            })?;

        let content_type = response
            .content_type()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        let payload = response.body.collect().await.map_err(|error| {
            storage_error(
                "get_object_body",
                &self.config.bucket,
                Some(&object_key),
                error,
            )
        })?;

        Ok((payload.into_bytes().to_vec(), content_type))
    }

    fn s3_client(&self) -> Client {
        build_s3_client(&self.config)
    }
}

impl MediaStorage for R2Storage {
    fn build_media_key(&self, note_id: &str, file_name: &str) -> Result<String> {
        let normalized_note_id = sanitize_token(note_id);
        if normalized_note_id.is_empty() {
            return Err(Error::InvalidInput(
                "Attachment note_id cannot be empty".to_string(),
            ));
        }

        let normalized_file_name = sanitize_file_name(file_name);
        let ts = Utc::now().timestamp_millis();
        let id = Uuid::now_v7();

        Ok(format!(
            "notes/{normalized_note_id}/{ts}-{id}-{normalized_file_name}"
        ))
    }

    fn public_object_url(&self, object_key: &str) -> Option<String> {
        let base = self.config.public_base_url.as_ref()?;
        let key = object_key.trim_matches('/');
        if key.is_empty() {
            return None;
        }

        Some(format!("{base}/{key}"))
    }
}

fn parse_config(lookup: impl Fn(&str) -> Option<String>) -> Result<Option<R2Config>> {
    let account_id = lookup(ENV_ACCOUNT_ID).map(|value| value.trim().to_string());
    let bucket = lookup(ENV_BUCKET).map(|value| value.trim().to_string());
    let access_key_id = lookup(ENV_ACCESS_KEY_ID).map(|value| value.trim().to_string());
    let secret_access_key = lookup(ENV_SECRET_ACCESS_KEY).map(|value| value.trim().to_string());
    let public_base_url = lookup(ENV_PUBLIC_BASE_URL).map(|value| value.trim().to_string());

    let any_present = account_id.is_some()
        || bucket.is_some()
        || access_key_id.is_some()
        || secret_access_key.is_some()
        || public_base_url.is_some();

    if !any_present {
        return Ok(None);
    }

    let mut missing = Vec::new();
    if account_id.as_ref().map_or(true, String::is_empty) {
        missing.push(ENV_ACCOUNT_ID);
    }
    if bucket.as_ref().map_or(true, String::is_empty) {
        missing.push(ENV_BUCKET);
    }
    if access_key_id.as_ref().map_or(true, String::is_empty) {
        missing.push(ENV_ACCESS_KEY_ID);
    }
    if secret_access_key.as_ref().map_or(true, String::is_empty) {
        missing.push(ENV_SECRET_ACCESS_KEY);
    }

    if !missing.is_empty() {
        return Err(Error::InvalidInput(format!(
            "R2 configuration is incomplete. Missing: {}",
            missing.join(", ")
        )));
    }

    let public_base_url = normalize_public_base_url(public_base_url)?;

    Ok(Some(R2Config {
        account_id: account_id.expect("validated above"),
        bucket: bucket.expect("validated above"),
        access_key_id: access_key_id.expect("validated above"),
        secret_access_key: secret_access_key.expect("validated above"),
        public_base_url,
    }))
}

fn build_s3_client(config: &R2Config) -> Client {
    let credentials = Credentials::new(
        config.access_key_id.clone(),
        config.secret_access_key.clone(),
        None,
        None,
        "dirt-core-r2-storage",
    );

    let sdk_config = aws_sdk_s3::config::Builder::new()
        .region(Region::new("auto"))
        .credentials_provider(credentials)
        .endpoint_url(config.endpoint_url())
        .force_path_style(true)
        .build();

    Client::from_conf(sdk_config)
}

fn storage_error(
    operation: &str,
    bucket: &str,
    object_key: Option<&str>,
    error: impl std::fmt::Display,
) -> Error {
    let target = object_key.map_or_else(|| bucket.to_string(), |key| format!("{bucket}/{key}"));
    Error::Storage(format!("R2 {operation} failed for {target}: {error}"))
}

fn normalize_object_key(object_key: &str) -> Result<String> {
    let object_key = object_key.trim().trim_matches('/').to_string();
    if object_key.is_empty() {
        return Err(Error::InvalidInput(
            "Attachment object_key cannot be empty".to_string(),
        ));
    }
    Ok(object_key)
}

fn normalize_content_type(content_type: Option<&str>) -> Option<String> {
    content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_public_base_url(public_base_url: Option<String>) -> Result<Option<String>> {
    let Some(value) = public_base_url else {
        return Ok(None);
    };

    if value.is_empty() {
        return Ok(None);
    }
    if !value.starts_with("https://") && !value.starts_with("http://") {
        return Err(Error::InvalidInput(
            "R2_PUBLIC_BASE_URL must start with http:// or https://".to_string(),
        ));
    }

    Ok(Some(value.trim_end_matches('/').to_string()))
}

fn sanitize_file_name(file_name: &str) -> String {
    let trimmed = file_name.trim().trim_matches('/');
    if trimmed.is_empty() {
        return "file".to_string();
    }

    let (stem, ext) = trimmed
        .rsplit_once('.')
        .map_or((trimmed, ""), |parts| parts);
    let stem = sanitize_token(stem);
    let stem = if stem.is_empty() {
        "file".to_string()
    } else {
        stem
    };
    let ext = sanitize_token(ext);

    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

fn sanitize_token(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;

    for ch in input.chars().flat_map(char::to_lowercase) {
        let keep = ch.is_ascii_alphanumeric();
        if keep {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }

    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::*;

    fn parse_from_map(map: &HashMap<&str, &str>) -> Result<Option<R2Config>> {
        parse_config(|key| map.get(key).map(|value| (*value).to_string()))
    }

    #[test]
    fn parse_config_none_returns_none() {
        let map = HashMap::new();
        assert!(parse_from_map(&map).unwrap().is_none());
    }

    #[test]
    fn parse_config_requires_all_required_values() {
        let mut map = HashMap::new();
        map.insert(ENV_ACCOUNT_ID, "account");
        map.insert(ENV_BUCKET, "bucket");

        let err = parse_from_map(&map).unwrap_err();
        match err {
            Error::InvalidInput(message) => {
                assert!(message.contains(ENV_ACCESS_KEY_ID));
                assert!(message.contains(ENV_SECRET_ACCESS_KEY));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parse_config_accepts_valid_values_and_normalizes_public_url() {
        let mut map = HashMap::new();
        map.insert(ENV_ACCOUNT_ID, "account-1");
        map.insert(ENV_BUCKET, "bucket-a");
        map.insert(ENV_ACCESS_KEY_ID, "AKID123");
        map.insert(ENV_SECRET_ACCESS_KEY, "SECRET123");
        map.insert(ENV_PUBLIC_BASE_URL, "https://cdn.example.com/media/");

        let config = parse_from_map(&map).unwrap().unwrap();
        assert_eq!(
            config.public_base_url.as_deref(),
            Some("https://cdn.example.com/media")
        );
        assert_eq!(
            config.endpoint_url(),
            "https://account-1.r2.cloudflarestorage.com"
        );
    }

    #[test]
    fn parse_config_rejects_invalid_public_base_url() {
        let mut map = HashMap::new();
        map.insert(ENV_ACCOUNT_ID, "account-1");
        map.insert(ENV_BUCKET, "bucket-a");
        map.insert(ENV_ACCESS_KEY_ID, "AKID123");
        map.insert(ENV_SECRET_ACCESS_KEY, "SECRET123");
        map.insert(ENV_PUBLIC_BASE_URL, "cdn.example.com/media");

        let err = parse_from_map(&map).unwrap_err();
        match err {
            Error::InvalidInput(message) => {
                assert!(message.contains("R2_PUBLIC_BASE_URL"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn build_media_key_normalizes_note_id_and_filename() {
        let storage = R2Storage::new(R2Config {
            account_id: "account-1".to_string(),
            bucket: "bucket-a".to_string(),
            access_key_id: "AKID123".to_string(),
            secret_access_key: "SECRET123".to_string(),
            public_base_url: Some("https://cdn.example.com/media".to_string()),
        });

        let key = storage
            .build_media_key(" NOTE::123 ", "My Photo (1).PNG")
            .unwrap();
        assert!(key.starts_with("notes/note-123/"));
        assert!(key.ends_with("-my-photo-1.png"));
    }

    #[test]
    fn public_object_url_joins_normalized_key() {
        let storage = R2Storage::new(R2Config {
            account_id: "account-1".to_string(),
            bucket: "bucket-a".to_string(),
            access_key_id: "AKID123".to_string(),
            secret_access_key: "SECRET123".to_string(),
            public_base_url: Some("https://cdn.example.com/media".to_string()),
        });

        let url = storage.public_object_url("/notes/note/file.png").unwrap();
        assert_eq!(url, "https://cdn.example.com/media/notes/note/file.png");
    }

    #[test]
    fn normalize_object_key_rejects_empty() {
        let err = normalize_object_key("   ").unwrap_err();
        match err {
            Error::InvalidInput(message) => assert!(message.contains("object_key")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn normalize_content_type_ignores_empty_values() {
        assert_eq!(normalize_content_type(None), None);
        assert_eq!(normalize_content_type(Some("   ")), None);
        assert_eq!(
            normalize_content_type(Some(" image/png ")),
            Some("image/png".to_string())
        );
    }

    #[test]
    #[ignore = "Requires local R2 env vars in process environment or .env"]
    fn from_env_loads_real_r2_config() {
        let _ = dotenvy::dotenv();

        let config = R2Config::from_env()
            .expect("R2 env parsing should not error")
            .expect("R2 config should be present");

        assert!(!config.account_id.trim().is_empty());
        assert!(!config.bucket.trim().is_empty());
        assert!(!config.access_key_id.trim().is_empty());
        assert!(!config.secret_access_key.trim().is_empty());
        assert_eq!(
            config.endpoint_url(),
            format!("https://{}.r2.cloudflarestorage.com", config.account_id)
        );

        if let Some(public_base_url) = config.public_base_url {
            assert!(
                public_base_url.starts_with("https://") || public_base_url.starts_with("http://")
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "Requires local R2 env vars plus network access"]
    async fn r2_bucket_exists_and_is_reachable() {
        let _ = dotenvy::dotenv();

        let config = R2Config::from_env()
            .expect("R2 env parsing should not error")
            .expect("R2 config should be present");
        let storage = R2Storage::new(config.clone());

        storage.bucket_is_reachable().await.unwrap_or_else(|error| {
            panic!(
                "R2 bucket health check failed for bucket '{}': {error}",
                config.bucket
            )
        });
    }

    async fn wait_for_object_state(storage: &R2Storage, object_key: &str, expected: bool) -> bool {
        for _attempt in 0..10 {
            let exists = storage
                .object_exists(object_key)
                .await
                .unwrap_or_else(|error| panic!("object existence check failed: {error}"));
            if exists == expected {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        false
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "Requires local R2 env vars plus network access"]
    async fn r2_object_roundtrip_upload_exists_delete() {
        let _ = dotenvy::dotenv();

        let config = R2Config::from_env()
            .expect("R2 env parsing should not error")
            .expect("R2 config should be present");
        let storage = R2Storage::new(config.clone());

        let object_key = storage
            .build_media_key("integration-note", "roundtrip.txt")
            .expect("media key generation should succeed");
        let bytes = b"r2-roundtrip-test";

        storage
            .upload_bytes(&object_key, bytes, Some("text/plain"))
            .await
            .unwrap_or_else(|error| panic!("R2 upload failed: {error}"));

        assert!(
            wait_for_object_state(&storage, &object_key, true).await,
            "Uploaded object was not observed in bucket '{}': {}",
            config.bucket,
            object_key
        );

        storage
            .delete_object(&object_key)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "R2 delete failed for bucket '{}' key '{}': {error}",
                    config.bucket, object_key
                )
            });

        assert!(
            wait_for_object_state(&storage, &object_key, false).await,
            "Deleted object still appears in bucket '{}': {}",
            config.bucket,
            object_key
        );
    }
}
