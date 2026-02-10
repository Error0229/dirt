use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::config::AppConfig;
use crate::error::AppError;

#[derive(Clone)]
pub struct EndpointRateLimiter {
    state: Arc<Mutex<HashMap<String, RateWindow>>>,
    window: Duration,
    sync_limit: u32,
    media_limit: u32,
    metrics: Arc<RateLimitMetrics>,
}

#[derive(Clone, Copy)]
pub enum ProtectedEndpoint {
    SyncToken,
    MediaPresign,
}

#[derive(Default)]
struct RateLimitMetrics {
    sync_allowed: AtomicU64,
    sync_limited: AtomicU64,
    media_allowed: AtomicU64,
    media_limited: AtomicU64,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct RateLimitMetricsSnapshot {
    pub sync_allowed: u64,
    pub sync_limited: u64,
    pub media_allowed: u64,
    pub media_limited: u64,
}

#[derive(Debug, Clone, Copy)]
struct RateWindow {
    started_at: Instant,
    count: u32,
}

impl EndpointRateLimiter {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            window: config.rate_limit_window,
            sync_limit: config.sync_token_rate_limit_per_window,
            media_limit: config.media_presign_rate_limit_per_window,
            metrics: Arc::new(RateLimitMetrics::default()),
        }
    }

    pub async fn check(&self, endpoint: ProtectedEndpoint, user_id: &str) -> Result<(), AppError> {
        let limit = match endpoint {
            ProtectedEndpoint::SyncToken => self.sync_limit,
            ProtectedEndpoint::MediaPresign => self.media_limit,
        };

        let key = format!("{}:{user_id}", endpoint.label());
        let now = Instant::now();
        let mut guard = self.state.lock().await;
        let entry = guard.entry(key).or_insert(RateWindow {
            started_at: now,
            count: 0,
        });

        if now.duration_since(entry.started_at) >= self.window {
            entry.started_at = now;
            entry.count = 0;
        }

        if entry.count >= limit {
            let retry_after_secs = self
                .window
                .saturating_sub(now.duration_since(entry.started_at))
                .as_secs();
            self.mark_limited(endpoint);
            tracing::warn!(
                endpoint = endpoint.label(),
                user = user_fingerprint(user_id),
                retry_after_secs,
                "Rate limit exceeded"
            );
            return Err(AppError::too_many_requests(
                "Rate limit exceeded for protected endpoint",
                retry_after_secs,
            ));
        }

        entry.count += 1;
        self.mark_allowed(endpoint);
        Ok(())
    }

    pub fn metrics_snapshot(&self) -> RateLimitMetricsSnapshot {
        RateLimitMetricsSnapshot {
            sync_allowed: self.metrics.sync_allowed.load(Ordering::Relaxed),
            sync_limited: self.metrics.sync_limited.load(Ordering::Relaxed),
            media_allowed: self.metrics.media_allowed.load(Ordering::Relaxed),
            media_limited: self.metrics.media_limited.load(Ordering::Relaxed),
        }
    }

    fn mark_allowed(&self, endpoint: ProtectedEndpoint) {
        match endpoint {
            ProtectedEndpoint::SyncToken => {
                self.metrics.sync_allowed.fetch_add(1, Ordering::Relaxed);
            }
            ProtectedEndpoint::MediaPresign => {
                self.metrics.media_allowed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn mark_limited(&self, endpoint: ProtectedEndpoint) {
        match endpoint {
            ProtectedEndpoint::SyncToken => {
                self.metrics.sync_limited.fetch_add(1, Ordering::Relaxed);
            }
            ProtectedEndpoint::MediaPresign => {
                self.metrics.media_limited.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl ProtectedEndpoint {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SyncToken => "sync_token",
            Self::MediaPresign => "media_presign",
        }
    }
}

fn user_fingerprint(user_id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    user_id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_blocks_after_limit() {
        let limiter = EndpointRateLimiter {
            state: Arc::new(Mutex::new(HashMap::new())),
            window: Duration::from_secs(60),
            sync_limit: 2,
            media_limit: 2,
            metrics: Arc::new(RateLimitMetrics::default()),
        };

        limiter
            .check(ProtectedEndpoint::SyncToken, "user-a")
            .await
            .unwrap();
        limiter
            .check(ProtectedEndpoint::SyncToken, "user-a")
            .await
            .unwrap();

        let err = limiter
            .check(ProtectedEndpoint::SyncToken, "user-a")
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::TooManyRequests(_, _)));

        let metrics = limiter.metrics_snapshot();
        assert_eq!(metrics.sync_allowed, 2);
        assert_eq!(metrics.sync_limited, 1);
    }
}
