//! Runtime configuration loaded from environment variables.

use std::env;
use std::fmt;

use crate::error::AppError;

/// Application configuration.
#[derive(Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub turso_database_url: String,
    /// Turso platform auth token. NEVER include in Debug output —
    /// leaks here end up in log aggregators, Vercel build logs, and
    /// crash dumps.
    pub turso_auth_token: String,
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppConfig")
            .field("bind_addr", &self.bind_addr)
            .field("turso_database_url", &self.turso_database_url)
            .field("turso_auth_token", &"[REDACTED]")
            .finish()
    }
}

impl AppConfig {
    /// Load configuration from environment variables.
    ///
    /// Required:
    ///   - `TURSO_DATABASE_URL`
    ///   - `TURSO_AUTH_TOKEN`
    ///
    /// Optional:
    ///   - `DIRT_API_BIND_ADDR` (default `0.0.0.0:8080` — Vercel ignores
    ///     this; only used by the local dev binary).
    ///
    /// Phase 1's `DIRT_SERVER_TOKEN` was removed in P2.2: the
    /// notes routes now require a per-user session token minted by
    /// `/v1/auth/verify`, not a shared server-wide bearer.
    pub fn from_env() -> Result<Self, AppError> {
        let turso_database_url = require_env("TURSO_DATABASE_URL")?;
        let turso_auth_token = require_env("TURSO_AUTH_TOKEN")?;
        let bind_addr = env::var("DIRT_API_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());

        Ok(Self {
            bind_addr,
            turso_database_url,
            turso_auth_token,
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
