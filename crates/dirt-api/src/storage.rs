//! The object storage that holds encrypted files.
//!
//! The server never handles a file's bytes. A device sends them straight to
//! the storage and fetches them straight from it, through links this module
//! signs. The only requests the server makes itself are to ask whether an
//! object is there and how large it is, and to delete one.
//!
//! Everything here is the S3 protocol and nothing is one provider's:
//! Cloudflare R2 in production, a gateway from `compose.yaml` in the tests.

use std::time::Duration;

use dirt_proto::api::limits;
use reqwest::header::CONTENT_LENGTH;
use reqwest::StatusCode;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use uuid::Uuid;

use crate::config::FilesConfig;

/// How long one of the server's own requests to the storage may take, and
/// how long the link it signs for that request works.
const OWN_REQUEST: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("the file storage is not set up correctly: {0}")]
    Setup(String),
    #[error("the file storage could not be reached: {0}")]
    Unreachable(String),
    #[error("the file storage answered {0}")]
    Refused(u16),
}

pub struct FileStore {
    bucket: Bucket,
    credentials: Credentials,
    http: reqwest::Client,
}

impl FileStore {
    pub fn new(config: &FilesConfig) -> Result<Self, StorageError> {
        let endpoint = config.endpoint.parse().map_err(|error| {
            StorageError::Setup(format!("the endpoint is not an address: {error}"))
        })?;
        // The bucket is part of the path, not of the host name. Both R2 and
        // the test gateway take it there, and it works with an address
        // that is an IP number.
        let bucket = Bucket::new(
            endpoint,
            UrlStyle::Path,
            config.bucket.clone(),
            config.region.clone(),
        )
        .map_err(|error| StorageError::Setup(error.to_string()))?;
        let http = reqwest::Client::builder()
            .timeout(OWN_REQUEST)
            .build()
            .map_err(|error| StorageError::Setup(error.to_string()))?;
        Ok(Self {
            bucket,
            credentials: Credentials::new(
                config.access_key_id.clone(),
                config.secret_access_key.clone(),
            ),
            http,
        })
    }

    /// Where a file is kept. One account's files share a prefix, and no
    /// link is ever signed for a key outside the caller's own.
    #[must_use]
    pub fn object_key(account: Uuid, file: Uuid) -> String {
        format!("{account}/{file}")
    }

    /// A link for one `PUT` of exactly `size` bytes.
    ///
    /// The length is one of the headers the signature covers. A request
    /// with any other `Content-Length` does not match the signature, so a
    /// device cannot store more than it asked for and was counted for.
    #[must_use]
    pub fn upload_link(&self, key: &str, size: i64) -> String {
        let mut action = self.bucket.put_object(Some(&self.credentials), key);
        action
            .headers_mut()
            .insert("content-length", size.to_string());
        action
            .sign(Duration::from_secs(u64::from(limits::UPLOAD_LINK_SECS)))
            .to_string()
    }

    /// A link for one `GET` of the object.
    #[must_use]
    pub fn download_link(&self, key: &str) -> String {
        self.bucket
            .get_object(Some(&self.credentials), key)
            .sign(Duration::from_secs(u64::from(limits::DOWNLOAD_LINK_SECS)))
            .to_string()
    }

    /// The size of the object, or `None` if the storage holds none.
    pub async fn size_of(&self, key: &str) -> Result<Option<i64>, StorageError> {
        let url = self
            .bucket
            .head_object(Some(&self.credentials), key)
            .sign(OWN_REQUEST);
        let response = self
            .http
            .head(url)
            .send()
            .await
            .map_err(|error| StorageError::Unreachable(error.to_string()))?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => {
                // Read from the header: the body of an answer to `HEAD` is
                // empty, whatever length it announces.
                let size = response
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<i64>().ok());
                size.map(Some).ok_or(StorageError::Refused(status.as_u16()))
            }
            status => Err(StorageError::Refused(status.as_u16())),
        }
    }

    /// Deletes the object. One that is already gone counts as deleted.
    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let url = self
            .bucket
            .delete_object(Some(&self.credentials), key)
            .sign(OWN_REQUEST);
        let response = self
            .http
            .delete(url)
            .send()
            .await
            .map_err(|error| StorageError::Unreachable(error.to_string()))?;
        match response.status() {
            status if status.is_success() || status == StatusCode::NOT_FOUND => Ok(()),
            status => Err(StorageError::Refused(status.as_u16())),
        }
    }

    /// Makes the bucket if it is not there. For a local store and the
    /// tests: in production the bucket is made by its owner.
    pub async fn create_bucket(&self) -> Result<(), StorageError> {
        let url = self
            .bucket
            .create_bucket(&self.credentials)
            .sign(OWN_REQUEST);
        let response = self
            .http
            .put(url)
            .header(CONTENT_LENGTH, 0)
            .send()
            .await
            .map_err(|error| StorageError::Unreachable(error.to_string()))?;
        match response.status() {
            status if status.is_success() || status == StatusCode::CONFLICT => Ok(()),
            status => Err(StorageError::Refused(status.as_u16())),
        }
    }
}
