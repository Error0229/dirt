//! dirt-api: the Dirt server.
//!
//! It does five things: checks a Clerk sign-in once per device and issues
//! that device a token; keeps the registry of devices; relays the exchange
//! by which one device hands the account key to another, and keeps the
//! optional recovery copy of that key, without being able to read either;
//! stores each note as an encrypted blob that it cannot read, replacing it
//! only when the writer saw the version being replaced; and enforces limits.
//!
//! It sends no email, merges nothing and never inspects a note. The protocol
//! types live in `dirt-proto`, shared with the clients.

pub mod account;
pub mod auth;
pub mod clerk;
pub mod config;
pub mod devices;
pub mod enrolments;
pub mod error;
pub mod notes;
pub mod recovery;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, FromRequest, Request};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use dirt_proto::api::{ErrorCode, PROTOCOL_HEADER, PROTOCOL_VERSION};
use serde::de::DeserializeOwned;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

pub use clerk::Keys;
pub use config::Config;
pub use error::{ApiError, ApiResult};

/// The schema, applied by `dirt-api migrate` and by the tests.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// The largest request body the server reads. A full push batch is about
/// 4 MB once its blobs are base64, and the hosting platform stops at 4.5 MB.
const BODY_LIMIT: usize = 4_500_000;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub keys: Arc<Keys>,
}

impl AppState {
    /// The state for a real server: a pool that connects on first use, and
    /// signing keys fetched from the identity provider on first use.
    pub fn from_config(config: Config) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            pool: lazy_pool(&config.database_url)?,
            keys: Arc::new(Keys::remote(&config.clerk_issuer)?),
            config: Arc::new(config),
        })
    }
}

/// Builds the pool without connecting, so that starting an instance costs
/// nothing when the request needs no database.
///
/// On Vercel each concurrent request gets its own instance, so the pool is
/// small: what limits connections is the number of instances, and Neon's
/// pooled endpoint is what absorbs them. Idle connections are dropped after
/// ten seconds because an instance is frozen between requests.
pub fn lazy_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    Ok(PgPoolOptions::new()
        .max_connections(3)
        .min_connections(0)
        .idle_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(15))
        .connect_lazy_with(database_url.parse()?))
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/nonces", post(devices::create_nonce))
        .route("/v1/devices", post(devices::register).get(devices::list))
        .route("/v1/devices/me", get(devices::me))
        .route("/v1/devices/{id}", delete(devices::revoke))
        .route(
            "/v1/enrolments/{device_id}/messages",
            post(enrolments::post_message).get(enrolments::messages),
        )
        .route(
            "/v1/enrolments/{device_id}/key",
            put(enrolments::put_key).get(enrolments::get_key),
        )
        .route(
            "/v1/enrolments/{device_id}/complete",
            post(enrolments::complete),
        )
        .route(
            "/v1/recovery",
            put(recovery::put)
                .get(recovery::get)
                .delete(recovery::delete),
        )
        .route("/v1/recovery/claim", post(recovery::claim))
        .route("/v1/account", delete(account::delete))
        .route(
            "/v1/account/reset",
            post(account::reset).delete(account::cancel_reset),
        )
        .route("/v1/notes/push", post(notes::push))
        .route("/v1/notes/pull", get(notes::pull))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn(check_protocol))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Liveness. Touches no database, so it answers even when Postgres is asleep.
async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "protocol": PROTOCOL_VERSION }))
}

async fn not_found() -> ApiError {
    ApiError::new(ErrorCode::NotFound, "no such endpoint")
}

/// Refuses a client that speaks a newer protocol than this server, before
/// anything else looks at the request. A client that sends no version is
/// taken to speak this one.
async fn check_protocol(request: Request, next: Next) -> Response {
    if let Some(value) = request.headers().get(PROTOCOL_HEADER) {
        match value
            .to_str()
            .ok()
            .and_then(|text| text.parse::<u32>().ok())
        {
            Some(version) if version <= PROTOCOL_VERSION => {}
            Some(version) => {
                return ApiError::new(
                    ErrorCode::UnsupportedProtocol,
                    format!(
                        "this server speaks protocol {PROTOCOL_VERSION}; the client sent {version}"
                    ),
                )
                .into_response();
            }
            None => {
                return ApiError::invalid("the protocol version must be a number").into_response();
            }
        }
    }
    next.run(request).await
}

/// A JSON request body. Unlike axum's own extractor, a body that cannot be
/// read is refused in the protocol's error format.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(json_rejection(&rejection)),
        }
    }
}

fn json_rejection(rejection: &JsonRejection) -> ApiError {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::new(ErrorCode::PayloadTooLarge, "the request body is too large")
    } else {
        ApiError::invalid(rejection.body_text())
    }
}
