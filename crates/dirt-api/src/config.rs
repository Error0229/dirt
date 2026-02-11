use std::collections::HashMap;
use std::env;
use std::fmt;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    MissingVar(&'static str),
    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

#[derive(Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub supabase_url: String,
    pub supabase_anon_key: String,
    pub supabase_jwks_url: String,
    pub supabase_jwt_issuer: String,
    pub supabase_jwt_audience: String,
    pub jwks_cache_ttl: Duration,
    pub bootstrap_manifest_version: String,
    pub bootstrap_cache_max_age_secs: u64,
    pub bootstrap_public_api_base_url: Option<String>,
    pub turso_api_url: String,
    pub turso_organization_slug: String,
    pub turso_database_name: String,
    pub turso_database_url: String,
    pub turso_platform_api_token: String,
    pub turso_token_ttl: Duration,
    pub media_url_ttl: Duration,
    pub auth_clock_skew: Duration,
    pub rate_limit_window: Duration,
    pub sync_token_rate_limit_per_window: u32,
    pub media_presign_rate_limit_per_window: u32,
    pub r2: Option<R2RuntimeConfig>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct R2RuntimeConfig {
    pub account_id: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl fmt::Debug for R2RuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("R2RuntimeConfig")
            .field("account_id", &self.account_id)
            .field("bucket", &self.bucket)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfig")
            .field("bind_addr", &self.bind_addr)
            .field("supabase_url", &self.supabase_url)
            .field("supabase_anon_key", &self.supabase_anon_key)
            .field("supabase_jwks_url", &self.supabase_jwks_url)
            .field("supabase_jwt_issuer", &self.supabase_jwt_issuer)
            .field("supabase_jwt_audience", &self.supabase_jwt_audience)
            .field("jwks_cache_ttl", &self.jwks_cache_ttl)
            .field(
                "bootstrap_manifest_version",
                &self.bootstrap_manifest_version,
            )
            .field(
                "bootstrap_cache_max_age_secs",
                &self.bootstrap_cache_max_age_secs,
            )
            .field(
                "bootstrap_public_api_base_url",
                &self.bootstrap_public_api_base_url,
            )
            .field("turso_api_url", &self.turso_api_url)
            .field("turso_organization_slug", &self.turso_organization_slug)
            .field("turso_database_name", &self.turso_database_name)
            .field("turso_database_url", &self.turso_database_url)
            .field("turso_platform_api_token", &"[REDACTED]")
            .field("turso_token_ttl", &self.turso_token_ttl)
            .field("media_url_ttl", &self.media_url_ttl)
            .field("auth_clock_skew", &self.auth_clock_skew)
            .field("rate_limit_window", &self.rate_limit_window)
            .field(
                "sync_token_rate_limit_per_window",
                &self.sync_token_rate_limit_per_window,
            )
            .field(
                "media_presign_rate_limit_per_window",
                &self.media_presign_rate_limit_per_window,
            )
            .field("r2", &self.r2)
            .finish()
    }
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let values: HashMap<String, String> = env::vars().collect();
        Self::from_lookup(|name| values.get(name).cloned())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind_addr = value_or_default(&lookup, "DIRT_API_BIND_ADDR", "127.0.0.1:8080");

        let supabase_url = required_trimmed(&lookup, "SUPABASE_URL")?;
        let supabase_anon_key = required_trimmed(&lookup, "SUPABASE_ANON_KEY")?;
        if !is_http_url(&supabase_url) {
            return Err(ConfigError::Invalid(
                "SUPABASE_URL must start with http:// or https://".to_string(),
            ));
        }

        let default_jwks = format!(
            "{}/auth/v1/.well-known/jwks.json",
            trim_trailing(&supabase_url)
        );
        let supabase_jwks_url = value_or_default(&lookup, "SUPABASE_JWKS_URL", &default_jwks);
        if !is_http_url(&supabase_jwks_url) {
            return Err(ConfigError::Invalid(
                "SUPABASE_JWKS_URL must start with http:// or https://".to_string(),
            ));
        }

        let default_issuer = format!("{}/auth/v1", trim_trailing(&supabase_url));
        let supabase_jwt_issuer = value_or_default(&lookup, "SUPABASE_JWT_ISSUER", &default_issuer);
        let supabase_jwt_audience =
            value_or_default(&lookup, "SUPABASE_JWT_AUDIENCE", "authenticated");

        let jwks_cache_ttl_secs = value_or_default(&lookup, "SUPABASE_JWKS_CACHE_TTL_SECS", "300")
            .parse::<u64>()
            .map_err(|_| {
                ConfigError::Invalid(
                    "SUPABASE_JWKS_CACHE_TTL_SECS must be an integer >= 30".to_string(),
                )
            })?;
        if jwks_cache_ttl_secs < 30 {
            return Err(ConfigError::Invalid(
                "SUPABASE_JWKS_CACHE_TTL_SECS must be >= 30".to_string(),
            ));
        }

        let bootstrap_manifest_version =
            value_or_default(&lookup, "BOOTSTRAP_MANIFEST_VERSION", "1");

        let bootstrap_cache_max_age_secs =
            value_or_default(&lookup, "BOOTSTRAP_CACHE_MAX_AGE_SECS", "300")
                .parse::<u64>()
                .map_err(|_| {
                    ConfigError::Invalid(
                        "BOOTSTRAP_CACHE_MAX_AGE_SECS must be an integer in [0, 86400]".to_string(),
                    )
                })?;
        if bootstrap_cache_max_age_secs > 86_400 {
            return Err(ConfigError::Invalid(
                "BOOTSTRAP_CACHE_MAX_AGE_SECS must be in [0, 86400]".to_string(),
            ));
        }

        let bootstrap_public_api_base_url =
            optional_trimmed(&lookup, "BOOTSTRAP_PUBLIC_API_BASE_URL")
                .map(|value| trim_trailing(&value).to_string());
        if let Some(url) = bootstrap_public_api_base_url.as_deref() {
            if !is_http_url(url) {
                return Err(ConfigError::Invalid(
                    "BOOTSTRAP_PUBLIC_API_BASE_URL must start with http:// or https://".to_string(),
                ));
            }
        }

        let turso_api_url = value_or_default(&lookup, "TURSO_API_URL", "https://api.turso.tech");
        if !is_http_url(&turso_api_url) {
            return Err(ConfigError::Invalid(
                "TURSO_API_URL must start with http:// or https://".to_string(),
            ));
        }

        let turso_organization_slug = required_trimmed(&lookup, "TURSO_ORGANIZATION_SLUG")?;
        let turso_database_name = required_trimmed(&lookup, "TURSO_DATABASE_NAME")?;
        let turso_database_url = required_trimmed(&lookup, "TURSO_DATABASE_URL")?;
        let turso_platform_api_token = required_trimmed(&lookup, "TURSO_PLATFORM_API_TOKEN")?;

        let turso_ttl_secs = value_or_default(&lookup, "TURSO_SYNC_TOKEN_TTL_SECS", "900")
            .parse::<u64>()
            .map_err(|_| {
                ConfigError::Invalid(
                    "TURSO_SYNC_TOKEN_TTL_SECS must be an integer in [60, 3600]".to_string(),
                )
            })?;
        if !(60..=3_600).contains(&turso_ttl_secs) {
            return Err(ConfigError::Invalid(
                "TURSO_SYNC_TOKEN_TTL_SECS must be in [60, 3600]".to_string(),
            ));
        }

        let media_ttl_secs = value_or_default(&lookup, "MEDIA_SIGNED_URL_TTL_SECS", "600")
            .parse::<u64>()
            .map_err(|_| {
                ConfigError::Invalid(
                    "MEDIA_SIGNED_URL_TTL_SECS must be an integer in [60, 3600]".to_string(),
                )
            })?;
        if !(60..=3_600).contains(&media_ttl_secs) {
            return Err(ConfigError::Invalid(
                "MEDIA_SIGNED_URL_TTL_SECS must be in [60, 3600]".to_string(),
            ));
        }

        let auth_clock_skew_secs = value_or_default(&lookup, "AUTH_CLOCK_SKEW_SECS", "60")
            .parse::<u64>()
            .map_err(|_| {
                ConfigError::Invalid(
                    "AUTH_CLOCK_SKEW_SECS must be an integer in [0, 300]".to_string(),
                )
            })?;
        if auth_clock_skew_secs > 300 {
            return Err(ConfigError::Invalid(
                "AUTH_CLOCK_SKEW_SECS must be in [0, 300]".to_string(),
            ));
        }

        let rate_limit_window_secs = value_or_default(&lookup, "RATE_LIMIT_WINDOW_SECS", "60")
            .parse::<u64>()
            .map_err(|_| {
                ConfigError::Invalid(
                    "RATE_LIMIT_WINDOW_SECS must be an integer in [10, 3600]".to_string(),
                )
            })?;
        if !(10..=3_600).contains(&rate_limit_window_secs) {
            return Err(ConfigError::Invalid(
                "RATE_LIMIT_WINDOW_SECS must be in [10, 3600]".to_string(),
            ));
        }

        let sync_token_rate_limit_per_window =
            value_or_default(&lookup, "SYNC_TOKEN_RATE_LIMIT_PER_WINDOW", "20")
                .parse::<u32>()
                .map_err(|_| {
                    ConfigError::Invalid(
                        "SYNC_TOKEN_RATE_LIMIT_PER_WINDOW must be an integer in [1, 1000]"
                            .to_string(),
                    )
                })?;
        if !(1..=1_000).contains(&sync_token_rate_limit_per_window) {
            return Err(ConfigError::Invalid(
                "SYNC_TOKEN_RATE_LIMIT_PER_WINDOW must be in [1, 1000]".to_string(),
            ));
        }

        let media_presign_rate_limit_per_window =
            value_or_default(&lookup, "MEDIA_PRESIGN_RATE_LIMIT_PER_WINDOW", "120")
                .parse::<u32>()
                .map_err(|_| {
                    ConfigError::Invalid(
                        "MEDIA_PRESIGN_RATE_LIMIT_PER_WINDOW must be an integer in [1, 5000]"
                            .to_string(),
                    )
                })?;
        if !(1..=5_000).contains(&media_presign_rate_limit_per_window) {
            return Err(ConfigError::Invalid(
                "MEDIA_PRESIGN_RATE_LIMIT_PER_WINDOW must be in [1, 5000]".to_string(),
            ));
        }

        let r2 = parse_r2_config(&lookup)?;

        Ok(Self {
            bind_addr,
            supabase_url,
            supabase_anon_key,
            supabase_jwks_url,
            supabase_jwt_issuer,
            supabase_jwt_audience,
            jwks_cache_ttl: Duration::from_secs(jwks_cache_ttl_secs),
            bootstrap_manifest_version,
            bootstrap_cache_max_age_secs,
            bootstrap_public_api_base_url,
            turso_api_url,
            turso_organization_slug,
            turso_database_name,
            turso_database_url,
            turso_platform_api_token,
            turso_token_ttl: Duration::from_secs(turso_ttl_secs),
            media_url_ttl: Duration::from_secs(media_ttl_secs),
            auth_clock_skew: Duration::from_secs(auth_clock_skew_secs),
            rate_limit_window: Duration::from_secs(rate_limit_window_secs),
            sync_token_rate_limit_per_window,
            media_presign_rate_limit_per_window,
            r2,
        })
    }
}

fn parse_r2_config(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Option<R2RuntimeConfig>, ConfigError> {
    let account_id = optional_trimmed(&lookup, "R2_ACCOUNT_ID");
    let bucket = optional_trimmed(&lookup, "R2_BUCKET");
    let access_key_id = optional_trimmed(&lookup, "R2_ACCESS_KEY_ID");
    let secret_access_key = optional_trimmed(&lookup, "R2_SECRET_ACCESS_KEY");

    let any_set = account_id.is_some()
        || bucket.is_some()
        || access_key_id.is_some()
        || secret_access_key.is_some();
    if !any_set {
        return Ok(None);
    }

    let account_id = account_id.ok_or(ConfigError::MissingVar("R2_ACCOUNT_ID"))?;
    let bucket = bucket.ok_or(ConfigError::MissingVar("R2_BUCKET"))?;
    let access_key_id = access_key_id.ok_or(ConfigError::MissingVar("R2_ACCESS_KEY_ID"))?;
    let secret_access_key =
        secret_access_key.ok_or(ConfigError::MissingVar("R2_SECRET_ACCESS_KEY"))?;

    Ok(Some(R2RuntimeConfig {
        account_id,
        bucket,
        access_key_id,
        secret_access_key,
    }))
}

fn value_or_default(lookup: impl Fn(&str) -> Option<String>, name: &str, default: &str) -> String {
    optional_trimmed(lookup, name).unwrap_or_else(|| default.to_string())
}

fn required_trimmed(
    lookup: impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, ConfigError> {
    optional_trimmed(lookup, name).ok_or(ConfigError::MissingVar(name))
}

fn optional_trimmed(lookup: impl Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    lookup(name).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn trim_trailing(value: &str) -> &str {
    value.trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn config_requires_minimum_secrets() {
        let map: HashMap<&str, &str> = HashMap::new();
        let err = AppConfig::from_lookup(|key| map.get(key).map(|value| (*value).to_string()))
            .unwrap_err();
        assert!(err.to_string().contains("SUPABASE_URL"));
    }

    #[test]
    fn config_redacts_sensitive_debug_fields() {
        let mut map = HashMap::new();
        map.insert("SUPABASE_URL", "https://project.supabase.co");
        map.insert("SUPABASE_ANON_KEY", "public-anon-key");
        map.insert("TURSO_ORGANIZATION_SLUG", "org");
        map.insert("TURSO_DATABASE_NAME", "db");
        map.insert("TURSO_DATABASE_URL", "libsql://db.turso.io");
        map.insert("TURSO_PLATFORM_API_TOKEN", "sensitive-platform-token");
        map.insert("R2_ACCOUNT_ID", "acc");
        map.insert("R2_BUCKET", "bucket");
        map.insert("R2_ACCESS_KEY_ID", "access");
        map.insert("R2_SECRET_ACCESS_KEY", "sensitive-r2-secret");

        let config =
            AppConfig::from_lookup(|key| map.get(key).map(|value| (*value).to_string())).unwrap();

        let debug_output = format!("{config:?}");
        assert!(!debug_output.contains("sensitive-platform-token"));
        assert!(!debug_output.contains("sensitive-r2-secret"));
        assert!(debug_output.contains("[REDACTED]"));
    }
}
