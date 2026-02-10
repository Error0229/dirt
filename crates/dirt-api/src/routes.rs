use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
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
use crate::turso::{MintedSyncToken, TursoTokenBroker};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    jwt_verifier: Arc<SupabaseJwtVerifier>,
    turso_broker: Arc<TursoTokenBroker>,
    r2_presign: Option<Arc<R2PresignService>>,
}

impl AppState {
    pub fn from_config(config: Arc<AppConfig>) -> Self {
        Self {
            jwt_verifier: Arc::new(SupabaseJwtVerifier::new(config.clone())),
            turso_broker: Arc::new(TursoTokenBroker::new(config.clone())),
            r2_presign: R2PresignService::from_config(&config).map(Arc::new),
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
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        timestamp: Utc::now().timestamp(),
    })
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
    let token = state.turso_broker.mint_sync_token(&user.user_id).await?;
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
    _user: Extension<AuthenticatedUser>,
    Json(request): Json<UploadPresignRequest>,
) -> Result<Json<PresignResponse>, AppError> {
    let signer = state.r2_presign.as_ref().ok_or_else(|| {
        AppError::Config("R2 presign service is not configured on the backend".to_string())
    })?;
    let operation = signer
        .presign_upload(&request.object_key, request.content_type.as_deref())
        .await?;
    Ok(Json(PresignResponse { operation }))
}

async fn presign_download(
    State(state): State<AppState>,
    _user: Extension<AuthenticatedUser>,
    Query(query): Query<DownloadPresignQuery>,
) -> Result<Json<PresignResponse>, AppError> {
    let signer = state.r2_presign.as_ref().ok_or_else(|| {
        AppError::Config("R2 presign service is not configured on the backend".to_string())
    })?;
    let operation = signer.presign_download(&query.object_key).await?;
    Ok(Json(PresignResponse { operation }))
}

async fn presign_delete(
    State(state): State<AppState>,
    _user: Extension<AuthenticatedUser>,
    Json(request): Json<DeletePresignRequest>,
) -> Result<Json<PresignResponse>, AppError> {
    let signer = state.r2_presign.as_ref().ok_or_else(|| {
        AppError::Config("R2 presign service is not configured on the backend".to_string())
    })?;
    let operation = signer.presign_delete(&request.object_key).await?;
    Ok(Json(PresignResponse { operation }))
}
