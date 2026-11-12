//! Configuration, read once from the environment.

use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Config {
    /// Postgres connection string. In production this is Neon's pooled
    /// endpoint; migrations use the direct one (see `main.rs`).
    pub database_url: String,
    /// The Clerk instance that signs ID tokens, for example
    /// `https://clerk.example.com`.
    pub clerk_issuer: String,
    /// The client ID of the Clerk OAuth application the clients sign in
    /// through. An ID token issued for any other client is refused.
    pub clerk_client_id: String,
    /// Where `dirt-api serve` listens. Not used on Vercel.
    pub bind_addr: SocketAddr,
    /// The object storage that holds encrypted files, if this server has
    /// any. Without it the server syncs notes and refuses files.
    pub files: Option<FilesConfig>,
    /// What the daily request that deletes expired files must present. The
    /// hosting platform sends it; without one the request is refused.
    pub cron_secret: Option<String>,
}

/// Object storage that speaks the S3 protocol: Cloudflare R2 in production,
/// a gateway from `compose.yaml` for the tests.
#[derive(Clone)]
pub struct FilesConfig {
    /// The storage's address, without the bucket.
    pub endpoint: String,
    pub bucket: String,
    /// R2 calls its one region `auto`.
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl FilesConfig {
    /// The store that `compose.yaml` runs for the tests and for a server on
    /// a developer's machine. Each setting can be replaced from the
    /// environment, as CI does.
    #[must_use]
    pub fn local() -> Self {
        let or = |name: &'static str, default: &str| {
            optional(name).unwrap_or_else(|| default.to_owned())
        };
        Self {
            endpoint: or("FILES_S3_ENDPOINT", "http://127.0.0.1:59000"),
            bucket: or("FILES_S3_BUCKET", "dirt-files"),
            region: or("FILES_S3_REGION", "us-east-1"),
            access_key_id: or("FILES_S3_ACCESS_KEY_ID", "dirt"),
            secret_access_key: or("FILES_S3_SECRET_ACCESS_KEY", "dirt-local-files"),
        }
    }
}

impl std::fmt::Debug for FilesConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key and its secret are left out of anything that is logged.
        f.debug_struct("FilesConfig")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("the environment variable {0} is not set")]
    Missing(&'static str),
    #[error("the environment variable {name} is not valid: {reason}")]
    Invalid { name: &'static str, reason: String },
}

fn optional(name: &'static str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The file storage, if any of its settings is present. One present and
/// another missing is a mistake worth stopping for: a server that quietly
/// refused files would look like a server with no files to offer.
fn files_from_env() -> Result<Option<FilesConfig>, ConfigError> {
    const NAMES: [&str; 4] = [
        "FILES_S3_ENDPOINT",
        "FILES_S3_BUCKET",
        "FILES_S3_ACCESS_KEY_ID",
        "FILES_S3_SECRET_ACCESS_KEY",
    ];
    if NAMES.iter().all(|name| optional(name).is_none()) {
        return Ok(None);
    }
    Ok(Some(FilesConfig {
        endpoint: required(NAMES[0])?.trim_end_matches('/').to_owned(),
        bucket: required(NAMES[1])?,
        region: optional("FILES_S3_REGION").unwrap_or_else(|| "auto".to_owned()),
        access_key_id: required(NAMES[2])?,
        secret_access_key: required(NAMES[3])?,
    }))
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        _ => Err(ConfigError::Missing(name)),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_addr = std::env::var("DIRT_API_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
            .parse()
            .map_err(|error: std::net::AddrParseError| ConfigError::Invalid {
                name: "DIRT_API_BIND_ADDR",
                reason: error.to_string(),
            })?;
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            clerk_issuer: required("CLERK_ISSUER")?.trim_end_matches('/').to_owned(),
            clerk_client_id: required("CLERK_CLIENT_ID")?,
            bind_addr,
            files: files_from_env()?,
            cron_secret: optional("CRON_SECRET"),
        })
    }
}
