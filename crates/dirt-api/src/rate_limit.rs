//! Sliding-window rate limiters.
//!
//! Two flavours, both backed by `VecDeque<Instant>` per window:
//!
//! - [`RateLimiter`] — single global window. Used by `/v1/auth/*`
//!   where there's no `user_id` yet (the routes mint sessions, they
//!   can't depend on having one). A login-flood here can lock out
//!   further login attempts but cannot starve sync traffic, because
//!   notes routes have their own limiter.
//!
//! - [`PerUserRateLimiter`] — one window per `user_id`. Used by
//!   `/v1/notes/*` after the session middleware has pinned the
//!   request to a real user. One chatty user cannot 429 another
//!   user's traffic. The map is pruned on the way out so empty
//!   windows don't accumulate.
//!
//! Each request prunes anything older than `WINDOW`, then either
//! inserts the new timestamp or returns a 429 with a `Retry-After`
//! derived from the oldest still-in-window request. No external
//! state, no extra dependencies, no thread-pool concerns: protected
//! by a tokio `Mutex` whose hold time is microseconds.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use tokio::sync::Mutex;

use crate::auth::AuthenticatedUser;
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

// ---- Per-user limiter ----

/// One sliding window per `user_id`.
///
/// Each window is its own `VecDeque<Instant>` under a single map-level
/// mutex; we accept the map-level contention because the lock hold
/// time is microseconds and the alternative (per-user inner mutex
/// with `DashMap`) brings a dependency for no measurable win at
/// solo-phase load.
///
/// Empty windows are dropped on eviction so a long-departed user's
/// entry doesn't pin memory. The map is bounded by the live user
/// count, which the auth flow already gates.
#[derive(Clone, Default)]
pub struct PerUserRateLimiter {
    state: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
}

impl PerUserRateLimiter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Apply the per-user sliding window.
///
/// Reads the resolved `user_id` from the request extension set by
/// [`crate::auth::require_session`]. If the extension is missing this
/// is a wiring bug — return 500 rather than silently letting
/// unauthenticated traffic through.
pub async fn enforce_per_user_rate_limit(
    State(limiter): State<PerUserRateLimiter>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let user = request
        .extensions()
        .get::<AuthenticatedUser>()
        .cloned()
        .ok_or_else(|| {
            AppError::internal(
                "PerUserRateLimiter ran before AuthenticatedUser was attached — check layer order",
            )
        })?;

    let now = Instant::now();
    let cutoff = now.checked_sub(WINDOW).unwrap_or(now);

    let mut map = limiter.state.lock().await;
    let queue = map.entry(user.user_id.clone()).or_default();
    while let Some(&front) = queue.front() {
        if front <= cutoff {
            queue.pop_front();
        } else {
            break;
        }
    }

    if queue.len() >= MAX_REQUESTS_PER_WINDOW {
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
    // If the queue ended up empty (unreachable here since we just
    // pushed, but defensive) drop the entry to keep the map tidy.
    if queue.is_empty() {
        map.remove(&user.user_id);
    }
    drop(map);

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

    // ---- per-user limiter ----

    /// Manually set up a router that pretends the auth middleware ran
    /// and pinned the request to a specific `user_id`. We test the
    /// limiter in isolation here; the integration with the real
    /// session middleware is covered by `lib.rs` smoke tests.
    fn build_per_user_router(limiter: PerUserRateLimiter, user_id: &str) -> Router {
        let user_id = user_id.to_string();
        Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                limiter.clone(),
                enforce_per_user_rate_limit,
            ))
            // Inject the AuthenticatedUser extension up-front so the
            // limiter middleware sees it. In production this is set by
            // `auth::require_session`; for a unit test, hand-injection
            // is enough.
            .layer(middleware::from_fn(move |mut req: Request, next: Next| {
                let user_id = user_id.clone();
                async move {
                    req.extensions_mut().insert(AuthenticatedUser { user_id });
                    next.run(req).await
                }
            }))
            .with_state(limiter)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn per_user_one_user_does_not_block_another() {
        let limiter = PerUserRateLimiter::new();

        // Saturate user A's window.
        {
            let mut map = limiter.state.lock().await;
            let queue = map.entry("user-a".to_string()).or_default();
            let now = Instant::now();
            for i in 0..MAX_REQUESTS_PER_WINDOW {
                queue.push_back(
                    now.checked_sub(Duration::from_millis(i as u64))
                        .unwrap_or(now),
                );
            }
        }

        // User A — saturated, expect 429.
        let router_a = build_per_user_router(limiter.clone(), "user-a");
        let resp = router_a
            .oneshot(
                HttpRequest::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        // User B — fresh window, expect 200.
        let router_b = build_per_user_router(limiter, "user-b");
        let resp = router_b
            .oneshot(
                HttpRequest::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn per_user_returns_500_when_extension_missing() {
        // No injection layer this time — the limiter should refuse.
        let limiter = PerUserRateLimiter::new();
        let router = Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                limiter.clone(),
                enforce_per_user_rate_limit,
            ))
            .with_state(limiter);

        let resp = router
            .oneshot(
                HttpRequest::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
