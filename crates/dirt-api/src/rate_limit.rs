//! Sliding-window rate limiter for the bearer-authed endpoints.
//!
//! Solo phase has a single shared bearer token, so the meaningful unit
//! of throttling is "requests reaching this server" rather than per-user
//! accounting. A single in-process sliding window is enough — once we
//! grow to multiple tokens we'll key the window map by token hash, but
//! that's premature today.
//!
//! The window itself is a `VecDeque<Instant>` of recent request times.
//! Each request prunes anything older than `WINDOW`, then either inserts
//! the new timestamp or returns a 429 with a `Retry-After` derived from
//! the oldest still-in-window request. No external state, no extra
//! dependencies, no thread-pool concerns: protected by a tokio
//! `Mutex` whose hold time is microseconds.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use tokio::sync::Mutex;

use crate::error::AppError;

/// How far back the sliding window looks. Combined with the cap below
/// this approximates ten requests per second sustained, with bursts up
/// to the cap.
pub const WINDOW: Duration = Duration::from_secs(60);

/// Maximum requests allowed within `WINDOW`.
///
/// Sized for the auto-sync worker's expected load (one push + one pull
/// every 30 s plus post-mutation kicks) with comfortable headroom for
/// occasional bursts during heavy editing sessions.
pub const MAX_REQUESTS_PER_WINDOW: usize = 600;

#[derive(Clone, Default)]
pub struct RateLimiter {
    state: Arc<Mutex<VecDeque<Instant>>>,
}

impl RateLimiter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

pub async fn enforce_rate_limit(
    State(limiter): State<RateLimiter>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let now = Instant::now();
    let cutoff = now.checked_sub(WINDOW).unwrap_or(now);

    let mut queue = limiter.state.lock().await;
    while let Some(&front) = queue.front() {
        if front <= cutoff {
            queue.pop_front();
        } else {
            break;
        }
    }

    if queue.len() >= MAX_REQUESTS_PER_WINDOW {
        // The oldest in-window request leaves the window at
        // `oldest + WINDOW`. The client should wait at least that long
        // before retrying (rounded up to the nearest second so 0 doesn't
        // sneak through).
        let retry_after_secs = queue.front().map_or(1, |oldest| {
            let elapsed = now.saturating_duration_since(*oldest);
            WINDOW.saturating_sub(elapsed).as_secs().max(1)
        });
        return Err(AppError::rate_limited(
            format!(
                "exceeded {MAX_REQUESTS_PER_WINDOW} requests per {} s",
                WINDOW.as_secs()
            ),
            retry_after_secs,
        ));
    }

    queue.push_back(now);
    drop(queue);
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::middleware;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    fn build_router(limiter: RateLimiter) -> Router {
        Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                limiter.clone(),
                enforce_rate_limit,
            ))
            .with_state(limiter)
    }

    async fn hit(router: &Router) -> StatusCode {
        let resp = router
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        resp.status()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn under_cap_requests_pass() {
        let limiter = RateLimiter::new();
        let router = build_router(limiter);
        for _ in 0..10 {
            assert_eq!(hit(&router).await, StatusCode::OK);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn requests_above_cap_get_429() {
        let limiter = RateLimiter::new();
        let router = build_router(limiter.clone());

        // Pre-fill the queue to capacity by hand so the test doesn't
        // have to fire MAX_REQUESTS_PER_WINDOW real requests.
        {
            let mut queue = limiter.state.lock().await;
            let now = Instant::now();
            for i in 0..MAX_REQUESTS_PER_WINDOW {
                queue.push_back(
                    now.checked_sub(Duration::from_millis(i as u64))
                        .unwrap_or(now),
                );
            }
        }

        let resp = router
            .oneshot(
                HttpRequest::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(resp.headers().contains_key("retry-after"));
    }
}
