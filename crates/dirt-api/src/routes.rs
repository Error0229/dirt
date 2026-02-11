use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::http::header::{self, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::auth::{extract_bearer_token, AuthenticatedUser, SupabaseJwtVerifier};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::media::{PresignedOperation, R2PresignService};
use crate::rate_limit::{EndpointRateLimiter, ProtectedEndpoint, RateLimitMetricsSnapshot};
use crate::turso::{MintedSyncToken, TursoTokenBroker};

const BOOTSTRAP_SCHEMA_VERSION: u32 = 1;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    jwt_verifier: Arc<SupabaseJwtVerifier>,
    turso_broker: Arc<TursoTokenBroker>,
    r2_presign: Option<Arc<R2PresignService>>,
    endpoint_rate_limiter: Arc<EndpointRateLimiter>,
}

impl AppState {
    pub fn from_config(config: Arc<AppConfig>) -> Self {
        Self {
            jwt_verifier: Arc::new(SupabaseJwtVerifier::new(config.clone())),
            turso_broker: Arc::new(TursoTokenBroker::new(config.clone())),
            r2_presign: R2PresignService::from_config(&config).map(Arc::new),
            endpoint_rate_limiter: Arc::new(EndpointRateLimiter::from_config(config.as_ref())),
            config,
        }
    }
}

pub fn app_router(state: AppState) -> Router {
    let protected_routes = Router::new()
        .route("/sync/token", post(mint_sync_token))
        .route("/media/presign/upload", post(presign_upload))
        .route("/media/presign/download", get(presign_download))
        .route("/media/presign/delete", post(presign_delete))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/bootstrap", get(bootstrap_manifest))
        .nest("/v1", protected_routes)
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(Any)
                .allow_methods(Any),
        )
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    timestamp: i64,
    rate_limit: RateLimitMetricsSnapshot,
}

async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        timestamp: Utc::now().timestamp(),
        rate_limit: state.endpoint_rate_limiter.metrics_snapshot(),
    })
}

#[derive(Debug, Serialize)]
struct BootstrapFeatureFlags {
    managed_sync: bool,
    managed_media: bool,
}

#[derive(Debug, Serialize)]
struct BootstrapManifest {
    schema_version: u32,
    manifest_version: String,
    supabase_url: String,
    supabase_anon_key: String,
    api_base_url: String,
    turso_sync_token_endpoint: String,
    feature_flags: BootstrapFeatureFlags,
}

async fn bootstrap_manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let api_base_url = resolve_public_api_base_url(state.config.as_ref());
    let manifest = BootstrapManifest {
        schema_version: BOOTSTRAP_SCHEMA_VERSION,
        manifest_version: state.config.bootstrap_manifest_version.clone(),
        supabase_url: state.config.supabase_url.clone(),
        supabase_anon_key: state.config.supabase_anon_key.clone(),
        turso_sync_token_endpoint: format!("{api_base_url}/v1/sync/token"),
        api_base_url,
        feature_flags: BootstrapFeatureFlags {
            managed_sync: true,
            managed_media: state.r2_presign.is_some(),
        },
    };

    let payload = serde_json::to_vec(&manifest).map_err(|error| {
        AppError::internal(format!("Failed to serialize bootstrap manifest: {error}"))
    })?;
    let etag = build_etag(&payload);
    if if_none_match_hit(headers.get(header::IF_NONE_MATCH), &etag) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        apply_bootstrap_cache_headers(
            response.headers_mut(),
            &etag,
            state.config.bootstrap_cache_max_age_secs,
        );
        return Ok(response);
    }

    let mut response = (StatusCode::OK, payload).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    apply_bootstrap_cache_headers(
        response.headers_mut(),
        &etag,
        state.config.bootstrap_cache_max_age_secs,
    );
    Ok(response)
}

fn resolve_public_api_base_url(config: &AppConfig) -> String {
    config
        .bootstrap_public_api_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || format!("http://{}", config.bind_addr.trim_end_matches('/')),
            |value| value.trim_end_matches('/').to_string(),
        )
}

fn build_etag(payload: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    payload.hash(&mut hasher);
    format!("W/\"{:016x}\"", hasher.finish())
}

fn if_none_match_hit(header_value: Option<&HeaderValue>, etag: &str) -> bool {
    let Some(header_value) = header_value else {
        return false;
    };
    let Ok(raw) = header_value.to_str() else {
        return false;
    };
    raw.split(',')
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate == etag)
}

fn apply_bootstrap_cache_headers(headers: &mut HeaderMap, etag: &str, max_age_secs: u64) {
    if let Ok(value) = HeaderValue::from_str(etag) {
        headers.insert(header::ETAG, value);
    }

    let cache_control = format!("public, max-age={max_age_secs}, must-revalidate");
    if let Ok(value) = HeaderValue::from_str(&cache_control) {
        headers.insert(header::CACHE_CONTROL, value);
    }
}

async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = extract_bearer_token(request.headers())?;
    let user = state.jwt_verifier.verify_access_token(token).await?;
    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

async fn mint_sync_token(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<MintedSyncToken>, AppError> {
    state
        .endpoint_rate_limiter
        .check(ProtectedEndpoint::SyncToken, &user.user_id)
        .await?;

    let user_hash = user_fingerprint(&user.user_id);
    let token = state.turso_broker.mint_sync_token(&user.user_id).await?;
    tracing::info!(
        endpoint = "sync_token",
        user = user_hash,
        session = user.session_id.as_deref().unwrap_or("none"),
        expires_at = token.expires_at,
        "Issued managed sync token"
    );
    Ok(Json(token))
}

#[derive(Debug, Deserialize)]
struct UploadPresignRequest {
    object_key: String,
    content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeletePresignRequest {
    object_key: String,
}

#[derive(Debug, Deserialize)]
struct DownloadPresignQuery {
    object_key: String,
}

#[derive(Debug, Serialize)]
struct PresignResponse {
    operation: PresignedOperation,
}

async fn presign_upload(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(request): Json<UploadPresignRequest>,
) -> Result<Json<PresignResponse>, AppError> {
    state
        .endpoint_rate_limiter
        .check(ProtectedEndpoint::MediaPresign, &user.user_id)
        .await?;

    let user_hash = user_fingerprint(&user.user_id);
    let signer = state.r2_presign.as_ref().ok_or_else(|| {
        AppError::Config("R2 presign service is not configured on the backend".to_string())
    })?;
    let operation = signer
        .presign_upload(&request.object_key, request.content_type.as_deref())
        .await?;
    tracing::info!(
        endpoint = "media_presign_upload",
        user = user_hash,
        object_key_len = request.object_key.len(),
        "Issued presigned upload URL"
    );
    Ok(Json(PresignResponse { operation }))
}

async fn presign_download(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<DownloadPresignQuery>,
) -> Result<Json<PresignResponse>, AppError> {
    state
        .endpoint_rate_limiter
        .check(ProtectedEndpoint::MediaPresign, &user.user_id)
        .await?;

    let user_hash = user_fingerprint(&user.user_id);
    let signer = state.r2_presign.as_ref().ok_or_else(|| {
        AppError::Config("R2 presign service is not configured on the backend".to_string())
    })?;
    let operation = signer.presign_download(&query.object_key).await?;
    tracing::info!(
        endpoint = "media_presign_download",
        user = user_hash,
        object_key_len = query.object_key.len(),
        "Issued presigned download URL"
    );
    Ok(Json(PresignResponse { operation }))
}

async fn presign_delete(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(request): Json<DeletePresignRequest>,
) -> Result<Json<PresignResponse>, AppError> {
    state
        .endpoint_rate_limiter
        .check(ProtectedEndpoint::MediaPresign, &user.user_id)
        .await?;

    let user_hash = user_fingerprint(&user.user_id);
    let signer = state.r2_presign.as_ref().ok_or_else(|| {
        AppError::Config("R2 presign service is not configured on the backend".to_string())
    })?;
    let operation = signer.presign_delete(&request.object_key).await?;
    tracing::info!(
        endpoint = "media_presign_delete",
        user = user_hash,
        object_key_len = request.object_key.len(),
        "Issued presigned delete URL"
    );
    Ok(Json(PresignResponse { operation }))
}

fn user_fingerprint(user_id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    user_id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::to_bytes;
    use axum::http::header;
    use axum::http::HeaderMap;

    use super::*;
    use crate::config::{AppConfig, R2RuntimeConfig};

    fn test_config() -> AppConfig {
        AppConfig {
            bind_addr: "127.0.0.1:8080".to_string(),
            supabase_url: "https://example.supabase.co".to_string(),
            supabase_anon_key: "public-anon".to_string(),
            supabase_jwks_url: "https://example.supabase.co/auth/v1/.well-known/jwks.json"
                .to_string(),
            supabase_jwt_issuer: "https://example.supabase.co/auth/v1".to_string(),
            supabase_jwt_audience: "authenticated".to_string(),
            jwks_cache_ttl: Duration::from_secs(300),
            bootstrap_manifest_version: "v1".to_string(),
            bootstrap_cache_max_age_secs: 300,
            bootstrap_public_api_base_url: Some("https://api.example.com".to_string()),
            turso_api_url: "https://api.turso.tech".to_string(),
            turso_organization_slug: "org".to_string(),
            turso_database_name: "db".to_string(),
            turso_database_url: "libsql://db.turso.io".to_string(),
            turso_platform_api_token: "secret".to_string(),
            turso_token_ttl: Duration::from_secs(900),
            media_url_ttl: Duration::from_secs(600),
            auth_clock_skew: Duration::from_secs(60),
            rate_limit_window: Duration::from_secs(60),
            sync_token_rate_limit_per_window: 20,
            media_presign_rate_limit_per_window: 120,
            r2: None,
        }
    }

    #[tokio::test]
    async fn bootstrap_manifest_returns_schema_and_cache_headers() {
        let state = AppState::from_config(Arc::new(test_config()));
        let response = bootstrap_manifest(State(state), HeaderMap::new())
            .await
            .expect("bootstrap response");

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert!(headers.contains_key(header::ETAG));
        assert_eq!(
            headers
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("public, max-age=300, must-revalidate")
        );

        let body_bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let payload: serde_json::Value =
            serde_json::from_slice(&body_bytes).expect("valid bootstrap JSON");
        assert_eq!(
            payload
                .get("schema_version")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            payload
                .get("turso_sync_token_endpoint")
                .and_then(|v| v.as_str()),
            Some("https://api.example.com/v1/sync/token")
        );
    }

    #[tokio::test]
    async fn bootstrap_manifest_supports_if_none_match() {
        let state = AppState::from_config(Arc::new(test_config()));
        let first = bootstrap_manifest(State(state.clone()), HeaderMap::new())
            .await
            .expect("bootstrap response");
        let etag = first
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .expect("etag header")
            .to_string();

        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_str(&etag).expect("valid etag"),
        );
        let second = bootstrap_manifest(State(state), headers)
            .await
            .expect("304 response");
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    }

    #[test]
    fn resolve_public_api_base_url_falls_back_to_bind_addr() {
        let mut config = test_config();
        config.bootstrap_public_api_base_url = None;
        config.bind_addr = "0.0.0.0:9999".to_string();
        assert_eq!(
            resolve_public_api_base_url(&config),
            "http://0.0.0.0:9999".to_string()
        );
    }

    #[test]
    fn managed_media_feature_reflects_r2_config() {
        let mut config = test_config();
        config.r2 = Some(R2RuntimeConfig {
            account_id: "acc".to_string(),
            bucket: "bucket".to_string(),
            access_key_id: "access".to_string(),
            secret_access_key: "secret".to_string(),
        });
        let state = AppState::from_config(Arc::new(config));
        assert!(state.r2_presign.is_some());
    }
}
