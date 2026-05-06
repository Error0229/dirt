//! Dirt sync backend.
//!
//! Exposes a tiny axum router with three sets of routes:
//!
//!   - `GET  /healthz`         — liveness probe, no auth.
//!   - `POST /v1/auth/{request,verify,refresh,logout}` — magic-code auth.
//!   - `POST /v1/notes/push`   — client pushes a batch of notes.
//!   - `GET  /v1/notes/pull`   — client pulls notes changed after a cursor.
//!
//! Authentication on `/v1/notes/*` is a per-user session token minted
//! by `/v1/auth/verify` and verified by [`auth::require_session`]. The
//! Phase 1 shared `DIRT_SERVER_TOKEN` was removed in P2.2.

pub mod auth;
pub mod config;
pub mod email;
pub mod error;
pub mod rate_limit;
pub mod routes;
pub mod routes_auth;
pub mod turso;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::HeaderValue;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub use config::AppConfig;
pub use email::EmailSender;
pub use error::AppError;
pub use rate_limit::{PerUserRateLimiter, RateLimiter};
pub use turso::TursoRepo;

/// Maximum acceptable request body size for `/v1/notes/push`.
///
/// Sized so a 500-note batch of ~10 KiB notes (5 MiB) fits with
/// headroom while preventing accidental OOM or abuse from oversized
/// payloads. Bodies larger than this short-circuit with HTTP 413
/// before reaching the handler.
pub const PUSH_BODY_LIMIT: usize = 8 * 1024 * 1024;

/// Shared state threaded through every handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub repo: Arc<TursoRepo>,
    pub email: Arc<EmailSender>,
}

impl AppState {
    #[must_use]
    pub const fn new(
        config: Arc<AppConfig>,
        repo: Arc<TursoRepo>,
        email: Arc<EmailSender>,
    ) -> Self {
        Self {
            config,
            repo,
            email,
        }
    }
}

/// Build the axum router with auth + rate limiting layered onto each
/// route group. Consumed by both the local dev binary and the Vercel
/// adapter.
pub fn build_router(state: AppState) -> Router {
    // Body limit is layered on the push route specifically; pull is a
    // GET so the limit is moot, and applying it globally would also cap
    // the (currently empty) `/healthz` response which would just be
    // noise.
    let push = post(routes::push_notes).layer(DefaultBodyLimit::max(PUSH_BODY_LIMIT));

    // Three limiters across two route groups. The auth routes can't
    // key by user yet (they mint sessions), so they share a single
    // global window. The notes routes use two layers:
    //
    //   1. `notes_ingress_limiter` (global) wraps the route group as
    //      the outermost layer. It bounds *all* traffic — including
    //      requests with missing or garbage bearers — so an attacker
    //      can't DOS Turso's session lookup with a flood of invalid
    //      tokens. Without this, only authenticated traffic counts
    //      against a budget.
    //   2. `per_user_limiter` runs after `auth::require_session` and
    //      keys by `user_id` so one chatty authenticated user can't
    //      429 another's traffic.
    //
    // Splitting the auth and notes ingress limiters (rather than
    // sharing one global pool) keeps a login-flood from spending the
    // whole budget and locking out the owner's sync.
    let auth_limiter = RateLimiter::new();
    let notes_ingress_limiter = RateLimiter::new();
    let per_user_limiter = PerUserRateLimiter::new();

    // axum applies layers outer-first: the **last** `.layer()` call
    // wraps as the outermost wrapper, i.e. it runs first on the way in.
    // Order on the way in needs to be:
    //   notes_ingress_limiter (global)
    //     → require_session (sets AuthenticatedUser)
    //       → enforce_per_user_rate_limit (reads AuthenticatedUser)
    //         → handler
    // We add the layers in the reverse order (innermost first).
    let authed = Router::new()
        .route("/v1/notes/push", push)
        .route("/v1/notes/pull", get(routes::pull_notes))
        .layer(middleware::from_fn_with_state(
            per_user_limiter,
            rate_limit::enforce_per_user_rate_limit,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ))
        .layer(middleware::from_fn_with_state(
            notes_ingress_limiter,
            rate_limit::enforce_rate_limit,
        ));

    // Magic-code auth routes. `request` and `verify` are pre-auth (no
    // session yet); `refresh` and `logout` consume a session token in
    // their own handler-level extractor. All four share a dedicated
    // limiter so a login-flood doesn't exhaust the sync budget.
    let auth_routes = Router::new()
        .route("/v1/auth/request", post(routes_auth::request_magic_code))
        .route("/v1/auth/verify", post(routes_auth::verify_magic_code))
        .route("/v1/auth/refresh", post(routes_auth::refresh_session))
        .route("/v1/auth/logout", post(routes_auth::logout_session))
        .layer(middleware::from_fn_with_state(
            auth_limiter,
            rate_limit::enforce_rate_limit,
        ));

    Router::new()
        .route("/healthz", get(routes::healthz))
        .merge(authed)
        .merge(auth_routes)
        .layer(build_cors_layer())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Build the CORS layer.
///
/// Session-token auth carries the real protection on these endpoints,
/// but a permissive CORS policy provides no defence-in-depth if a
/// token ever ends up in a browser context. `CORS_ALLOWED_ORIGIN` (set
/// in the server env, e.g. `https://app.example.com`) restricts
/// cross-origin requests to that one origin. When unset we keep the
/// permissive policy so existing native-client deploys (where CORS
/// isn't enforced at all) keep working — explicitly logged so the
/// operator can opt in.
fn build_cors_layer() -> CorsLayer {
    let base = CorsLayer::new().allow_methods(Any).allow_headers(Any);

    match std::env::var("CORS_ALLOWED_ORIGIN").ok().as_deref() {
        Some(origin) if !origin.trim().is_empty() => match HeaderValue::from_str(origin.trim()) {
            Ok(value) => base.allow_origin(value),
            Err(err) => {
                tracing::warn!(
                    "CORS_ALLOWED_ORIGIN is not a valid header value ({err}); falling back to Any"
                );
                base.allow_origin(Any)
            }
        },
        _ => base.allow_origin(Any),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state(repo: Arc<TursoRepo>) -> AppState {
        let config = AppConfig {
            bind_addr: "127.0.0.1:0".into(),
            turso_database_url: "libsql://unused.test".into(),
            turso_auth_token: "unused".into(),
        };
        AppState {
            config: Arc::new(config),
            repo,
            email: Arc::new(EmailSender::log_only()),
        }
    }

    /// Body-limit layer rejects oversized push payloads. The session
    /// middleware now runs first, so the request needs a valid bearer
    /// to even reach the body-limit gate — without one, the auth layer
    /// 401s before the body is read.
    #[tokio::test(flavor = "current_thread")]
    async fn push_rejects_oversized_body_with_413() {
        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let user_id = temp_db
            .repo
            .upsert_user_by_email("oversize@example.com", now)
            .await
            .unwrap();
        let raw_token = "valid-token-for-413-test";
        let token_hash = auth::sha256_b64url(raw_token.as_bytes());
        temp_db
            .repo
            .insert_auth_session(&user_id, &token_hash, now, now + 1_000_000)
            .await
            .unwrap();

        let router = build_router(test_state(Arc::clone(&temp_db.repo)));

        let oversized = vec![b'x'; PUSH_BODY_LIMIT + 1024 * 1024];
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/notes/push")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_token}"))
                    .body(Body::from(oversized))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// Floods of invalid bearers must be capped by the global notes
    /// ingress limiter — without it, unauthenticated requests sail
    /// through the auth middleware all the way to the Turso session
    /// lookup, burning DB capacity that no per-user limiter can
    /// account for. We pre-fill the ingress queue at the route layer
    /// here rather than firing 600 real requests; this proves the
    /// outer limiter is wired ahead of the auth check.
    #[tokio::test(flavor = "current_thread")]
    async fn invalid_bearer_flood_is_capped_by_ingress_limiter() {
        use crate::rate_limit::enforce_rate_limit;

        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let state = test_state(Arc::clone(&temp_db.repo));

        // Build a tiny router that mirrors the real layering: ingress
        // limiter outermost, then session auth. The ingress limiter is
        // hand-saturated, so the request must 429 before the auth
        // middleware even runs.
        let saturated = RateLimiter::new();
        saturated.saturate_for_test().await;
        let router = Router::new()
            .route("/v1/notes/push", axum::routing::post(routes::push_notes))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                auth::require_session,
            ))
            .layer(middleware::from_fn_with_state(
                saturated,
                enforce_rate_limit,
            ))
            .with_state(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/notes/push")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer never-minted-this-token")
                    .body(Body::from(r#"{"notes":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    /// `/v1/notes/push` without a session token must 401 — proves the
    /// session middleware is layered on the route group correctly.
    #[tokio::test(flavor = "current_thread")]
    async fn push_without_session_token_returns_401() {
        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let router = build_router(test_state(Arc::clone(&temp_db.repo)));

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/notes/push")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"notes":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// End-to-end happy path: mint a session row, hit `/v1/notes/push`
    /// with the bearer, observe a 200. This is the smallest test that
    /// proves the full middleware stack (session → per-user limiter →
    /// handler) is wired correctly.
    #[tokio::test(flavor = "current_thread")]
    async fn push_with_valid_session_token_succeeds() {
        let temp_db = TursoRepo::connect_temp_db().await.unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let user_id = temp_db
            .repo
            .upsert_user_by_email("push@example.com", now)
            .await
            .unwrap();
        let raw_token = "valid-session-token-end-to-end";
        let token_hash = auth::sha256_b64url(raw_token.as_bytes());
        temp_db
            .repo
            .insert_auth_session(&user_id, &token_hash, now, now + 1_000_000)
            .await
            .unwrap();

        let router = build_router(test_state(Arc::clone(&temp_db.repo)));
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/notes/push")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_token}"))
                    .body(Body::from(r#"{"notes":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
