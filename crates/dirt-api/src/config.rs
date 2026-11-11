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
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("the environment variable {0} is not set")]
    Missing(&'static str),
    #[error("the environment variable {name} is not valid: {reason}")]
    Invalid { name: &'static str, reason: String },
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
        })
    }
}
