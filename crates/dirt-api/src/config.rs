//! Runtime configuration loaded from environment variables.

use std::env;
use std::fmt;

use crate::error::AppError;

/// Bearer token required on `/v1/*` routes.
///
/// Stored as a byte vector so the comparison path can use
/// `subtle::ConstantTimeEq` without re-decoding on every request.
#[derive(Clone)]
pub struct ServerToken(pub Vec<u8>);

impl fmt::Debug for ServerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ServerToken").field(&"[REDACTED]").finish()
    }
}

/// Application configuration.
#[derive(Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub turso_database_url: String,
    /// Turso platform auth token. NEVER include in Debug output —
    /// leaks here end up in log aggregators, Vercel build logs, and
    /// crash dumps.
    pub turso_auth_token: String,
    pub server_token: ServerToken,
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppConfig")
            .field("bind_addr", &self.bind_addr)
            .field("turso_database_url", &self.turso_database_url)
            .field("turso_auth_token", &"[REDACTED]")
            .field("server_token", &self.server_token)
            .finish()
    }
}

impl AppConfig {
    /// Load configuration from environment variables.
    ///
    /// Required:
    ///   - `TURSO_DATABASE_URL`
    ///   - `TURSO_AUTH_TOKEN`
    ///   - `DIRT_SERVER_TOKEN`
    ///
    /// Optional:
    ///   - `DIRT_API_BIND_ADDR` (default `0.0.0.0:8080` — Vercel ignores
    ///     this; only used by the local dev binary).
    pub fn from_env() -> Result<Self, AppError> {
        let turso_database_url = require_env("TURSO_DATABASE_URL")?;
        let turso_auth_token = require_env("TURSO_AUTH_TOKEN")?;
        let server_token_raw = require_env("DIRT_SERVER_TOKEN")?;
        if server_token_raw.len() < 32 {
            return Err(AppError::config(
                "DIRT_SERVER_TOKEN must be at least 32 characters \
                 (generate with: openssl rand -hex 32)",
            ));
        }
        let bind_addr = env::var("DIRT_API_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());

        Ok(Self {
            bind_addr,
            turso_database_url,
            turso_auth_token,
            server_token: ServerToken(server_token_raw.into_bytes()),
        })
    }
}

fn require_env(key: &str) -> Result<String, AppError> {
    let value =
        env::var(key).map_err(|_| AppError::config(format!("missing required env var: {key}")))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::config(format!("env var {key} must not be empty")));
    }
    // Trim before returning. `cat token.txt` and many .env loaders leave
    // a trailing newline; if the server stored the raw value, every
    // request would 401 because the client always trims.
    Ok(trimmed.to_string())
}
