//! Trial 4: the push and pull queries the real server will run, behind a
//! small axum router that the Vercel function and the tests both use.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use sqlx::PgPool;
use uuid::Uuid;

/// Compiler that built this binary, recorded by `build.rs`.
pub const RUSTC_VERSION: &str = env!("TRIAL_RUSTC_VERSION");

/// Larger than Vercel's request limit, so the platform's limit is the one the
/// body test observes, not axum's 2 MB default.
const BODY_LIMIT: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// Builds the pool without connecting. The first query opens the first
/// connection, so a cold function start does not pay for a database that the
/// request may not need.
///
/// `statement_cache` overrides sqlx's per-connection prepared-statement cache
/// size; `Some(0)` turns the cache off.
pub fn lazy_pool(url: &str, statement_cache: Option<usize>) -> Result<PgPool, sqlx::Error> {
    let mut options: PgConnectOptions = url.parse()?;
    options = options.ssl_mode(PgSslMode::Require);
    if let Some(capacity) = statement_cache {
        options = options.statement_cache_capacity(capacity);
    }
    Ok(PgPoolOptions::new()
        .max_connections(3)
        .min_connections(0)
        .idle_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(15))
        .connect_lazy_with(options))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// Stored. Holds the note's new change number.
    Accepted(i64),
    /// The note changed since the base the client saw, or the client named a
    /// base for a note the server does not have. Nothing was written.
    Stale,
    /// No such account. Nothing was written.
    NoAccount,
}

pub async fn create_account(pool: &PgPool, account: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO accounts (id) VALUES ($1) ON CONFLICT (id) DO NOTHING")
        .bind(account)
        .execute(pool)
        .await?;
    Ok(())
}

/// Stores one note if `base` is still the note's change number. `base` is 0
/// for a note the client believes is new.
///
/// The first statement locks the account row, so pushes to one account run
/// one at a time and take change numbers in commit order.
pub async fn push(
    pool: &PgPool,
    account: Uuid,
    note: Uuid,
    base: i64,
    ciphertext: &[u8],
) -> Result<PushOutcome, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let next: Option<i64> = sqlx::query_scalar(
        "UPDATE accounts SET change_counter = change_counter + 1 \
          WHERE id = $1 RETURNING change_counter",
    )
    .bind(account)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(next) = next else {
        tx.rollback().await?;
        return Ok(PushOutcome::NoAccount);
    };

    // The plan's statement used `VALUES`, which inserts a missing note
    // whatever base the client sent. Selecting the row only when the base is
    // 0 or the note exists closes that gap: a non-zero base for a missing
    // note is stale.
    let stored: Option<i64> = sqlx::query_scalar(
        "INSERT INTO notes (account_id, note_id, change_number, ciphertext) \
         SELECT $1::uuid, $2::uuid, $3::bigint, $4::bytea \
          WHERE $5::bigint = 0 \
             OR EXISTS (SELECT 1 FROM notes WHERE account_id = $1 AND note_id = $2) \
         ON CONFLICT (account_id, note_id) DO UPDATE \
            SET change_number = EXCLUDED.change_number, \
                ciphertext    = EXCLUDED.ciphertext \
          WHERE notes.change_number = $5 \
         RETURNING change_number",
    )
    .bind(account)
    .bind(note)
    .bind(next)
    .bind(ciphertext)
    .bind(base)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(change_number) = stored {
        tx.commit().await?;
        Ok(PushOutcome::Accepted(change_number))
    } else {
        // Rolling back returns the counter, so a rejected push leaves no gap
        // in the account's change numbers.
        tx.rollback().await?;
        Ok(PushOutcome::Stale)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PulledNote {
    pub note_id: Uuid,
    pub change_number: i64,
    pub bytes: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullPage {
    /// The account's counter when the page was read. `None` means no such
    /// account.
    pub change_counter: Option<i64>,
    /// When the database server process started, as Unix seconds. It changes
    /// each time Neon suspends and resumes the compute.
    pub database_started_at: f64,
    pub notes: Vec<PulledNote>,
}

/// Reads notes changed after `after`, oldest change first. The trial returns
/// sizes instead of the ciphertext itself; response size is measured
/// separately through `/blob`.
pub async fn pull(
    pool: &PgPool,
    account: Uuid,
    after: i64,
    limit: i64,
) -> Result<PullPage, sqlx::Error> {
    let (change_counter, database_started_at): (Option<i64>, f64) = sqlx::query_as(
        "SELECT (SELECT change_counter FROM accounts WHERE id = $1), \
                extract(epoch FROM pg_postmaster_start_time())::float8",
    )
    .bind(account)
    .fetch_one(pool)
    .await?;

    let rows: Vec<(Uuid, i64, i32)> = sqlx::query_as(
        "SELECT note_id, change_number, octet_length(ciphertext) \
           FROM notes \
          WHERE account_id = $1 AND change_number > $2 \
          ORDER BY change_number \
          LIMIT $3",
    )
    .bind(account)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(PullPage {
        change_counter,
        database_started_at,
        notes: rows
            .into_iter()
            .map(|(note_id, change_number, bytes)| PulledNote {
                note_id,
                change_number,
                bytes,
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    /// Shared secret for every route but `/healthz`. The deployment is public
    /// and the trial has no real sign-in.
    pub token: String,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/account", post(account_route))
        .route("/push", post(push_route))
        .route("/pull", get(pull_route))
        .route("/blob", get(blob_route))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    rustc: &'static str,
    region: Option<String>,
    deployment: Option<String>,
}

async fn healthz() -> Response {
    with_trial_headers(
        Json(Health {
            ok: true,
            rustc: RUSTC_VERSION,
            region: std::env::var("VERCEL_REGION").ok(),
            deployment: std::env::var("VERCEL_DEPLOYMENT_ID").ok(),
        })
        .into_response(),
        None,
    )
}

#[derive(Deserialize)]
struct AccountParams {
    id: Uuid,
}

async fn account_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<AccountParams>,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let started = Instant::now();
    let result = create_account(&state.pool, params.id).await;
    let elapsed = started.elapsed();
    match result {
        Ok(()) => with_trial_headers(StatusCode::NO_CONTENT.into_response(), Some(elapsed)),
        Err(error) => database_error(&error, elapsed),
    }
}

#[derive(Deserialize)]
struct PushParams {
    account: Uuid,
    note: Uuid,
    base: i64,
}

#[derive(Serialize, Deserialize)]
pub struct PushReply {
    pub status: String,
    pub change_number: Option<i64>,
    pub bytes: usize,
}

async fn push_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<PushParams>,
    body: Bytes,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let started = Instant::now();
    let result = push(&state.pool, params.account, params.note, params.base, &body).await;
    let elapsed = started.elapsed();
    let (status, name, change_number) = match result {
        Ok(PushOutcome::Accepted(n)) => (StatusCode::OK, "accepted", Some(n)),
        Ok(PushOutcome::Stale) => (StatusCode::CONFLICT, "stale", None),
        Ok(PushOutcome::NoAccount) => (StatusCode::NOT_FOUND, "no_account", None),
        Err(error) => return database_error(&error, elapsed),
    };
    let reply = PushReply {
        status: name.to_owned(),
        change_number,
        bytes: body.len(),
    };
    with_trial_headers((status, Json(reply)).into_response(), Some(elapsed))
}

#[derive(Deserialize)]
struct PullParams {
    account: Uuid,
    #[serde(default)]
    after: i64,
    #[serde(default = "default_limit")]
    limit: i64,
}

const fn default_limit() -> i64 {
    100
}

async fn pull_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<PullParams>,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let started = Instant::now();
    let result = pull(&state.pool, params.account, params.after, params.limit).await;
    let elapsed = started.elapsed();
    match result {
        Ok(page) => with_trial_headers(Json(page).into_response(), Some(elapsed)),
        Err(error) => database_error(&error, elapsed),
    }
}

#[derive(Deserialize)]
struct BlobParams {
    bytes: usize,
}

/// Returns a body of the requested size, to find the response size limit.
async fn blob_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<BlobParams>,
) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    if params.bytes > BODY_LIMIT {
        return StatusCode::BAD_REQUEST.into_response();
    }
    with_trial_headers(vec![b'a'; params.bytes].into_response(), None)
}

#[allow(clippy::result_large_err)]
fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let given = headers
        .get("x-trial-token")
        .and_then(|value| value.to_str().ok());
    if given == Some(state.token.as_str()) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED.into_response())
    }
}

/// A database failure, with the error text in the body. The load script
/// searches these bodies for prepared-statement errors.
fn database_error(error: &sqlx::Error, elapsed: Duration) -> Response {
    with_trial_headers(
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("database error: {error}"),
        )
            .into_response(),
        Some(elapsed),
    )
}

/// Adds headers that let the load script tell apart time spent in the
/// database from time spent on the network, and one deployment from another.
fn with_trial_headers(mut response: Response, database_time: Option<Duration>) -> Response {
    // One value per running copy of the function, so the load script can
    // count how many copies served a burst.
    static INSTANCE: OnceLock<String> = OnceLock::new();

    let headers = response.headers_mut();
    if let Some(elapsed) = database_time {
        let millis = format!("{:.2}", elapsed.as_secs_f64() * 1000.0);
        if let Ok(value) = HeaderValue::from_str(&millis) {
            headers.insert(HeaderName::from_static("x-db-ms"), value);
        }
    }
    let instance = INSTANCE.get_or_init(|| Uuid::new_v4().simple().to_string());
    if let Ok(value) = HeaderValue::from_str(instance) {
        headers.insert(HeaderName::from_static("x-trial-instance"), value);
    }
    for (name, variable) in [
        ("x-trial-deployment", "VERCEL_DEPLOYMENT_ID"),
        ("x-trial-region", "VERCEL_REGION"),
    ] {
        if let Some(value) = std::env::var(variable)
            .ok()
            .and_then(|text| HeaderValue::from_str(&text).ok())
        {
            headers.insert(HeaderName::from_static(name), value);
        }
    }
    response
}
