//! HTTP client for the dirt-api sync backend.
//!
//! Thin wrapper around the two bearer-authed endpoints exposed by
//! `dirt-api`:
//!
//!   - `POST /v1/notes/push` — accept local mutations, stamp
//!     `server_updated_at`.
//!   - `GET  /v1/notes/pull` — paginated incremental pull ordered by
//!     `(server_updated_at, id)`.
//!
//! Request/response shapes mirror those declared in `dirt-api::routes`.
//! Keeping them here (instead of re-exporting the server's types) avoids
//! a circular crate dependency and makes the wire contract explicit in
//! both directions.
//!
//! The client never touches `server_updated_at` directly — that field is
//! server-authoritative and only ever *read* by the merge resolver. On
//! the push path we transmit `client_updated_at_ms` (local wall clock)
//! and `deleted_at_ms`; the server returns the stamped value per id.
//!
//! Errors are bucketed so each client driver (desktop / mobile / cli)
//! can choose a retry strategy per variant without re-parsing HTTP
//! status codes. `Unauthorized` should surface in the UI so the user can
//! rotate `DIRT_CLIENT_TOKEN`; `ServerUnavailable` is safe to retry with
//! backoff; `BadRequest` means the request was malformed and retrying
//! unchanged will fail again.

use std::fmt;
use std::str::FromStr;

use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{Note, NoteId};
use crate::util::{is_http_url, normalize_text_option};
use crate::SOLO_USER_ID;

const PUSH_PATH: &str = "/v1/notes/push";
const PULL_PATH: &str = "/v1/notes/pull";

/// Errors returned by the sync API client.
#[derive(Debug, Error)]
pub enum ApiClientError {
    /// Base URL or bearer token was missing or malformed at construction time.
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),
    /// The request never reached the server (DNS, TLS, connection refused,
    /// timeout, etc.). Safe to retry with backoff.
    #[error("network error: {0}")]
    Network(String),
    /// Server replied 401. The bearer token on this client does not match
    /// the server's `DIRT_SERVER_TOKEN`. Retrying with the same token will
    /// keep failing — surface to the user.
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    /// Server replied 400 / 413. Request was malformed. `code` carries the
    /// dirt-api-specific code (e.g. `BATCH_TOO_LARGE`, `BAD_REQUEST`).
    #[error("bad request ({code}): {message}")]
    BadRequest { code: String, message: String },
    /// Server replied 503 (usually Turso reachability). Safe to retry.
    #[error("server unavailable: {0}")]
    ServerUnavailable(String),
    /// Any other non-2xx response. Includes the raw status so callers can
    /// distinguish 5xx server bugs from unexpected codes.
    #[error("server error ({status}): {message}")]
    ServerError { status: u16, message: String },
    /// Response body could not be decoded against the expected schema.
    /// Almost always a server/client contract drift.
    #[error("decode error: {0}")]
    Decode(String),
}

pub type ApiClientResult<T> = Result<T, ApiClientError>;

// ---- Wire types: mirror `dirt-api::routes` exactly ----

/// A note the client is pushing to the server. Matches
/// `dirt_api::routes::PushRequestNote`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushNote {
    pub id: String,
    pub content: String,
    pub created_at_ms: i64,
    pub client_updated_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PushRequestBody<'a> {
    notes: &'a [PushNote],
}

/// Server response to `POST /v1/notes/push`.
#[derive(Debug, Clone, Deserialize)]
pub struct PushResponse {
    pub results: Vec<PushResult>,
    pub server_time_ms: i64,
}

/// Per-note outcome stamped by the server.
#[derive(Debug, Clone, Deserialize)]
pub struct PushResult {
    pub id: String,
    pub applied: bool,
    pub server_updated_at_ms: i64,
}

/// Server response to `GET /v1/notes/pull`.
#[derive(Debug, Clone, Deserialize)]
pub struct PullResponse {
    pub notes: Vec<PullNote>,
    pub server_time_ms: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// A single note returned by pull. Matches `dirt_api::routes::PullNote`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PullNote {
    pub id: String,
    pub content: String,
    pub created_at_ms: i64,
    pub client_updated_at_ms: i64,
    pub server_updated_at_ms: i64,
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
}

// ---- Server error envelope (matches `dirt_api::error::ErrorEnvelope`) ----

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

// ---- Client ----

/// HTTP client bound to a specific backend URL + bearer token.
#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    token: String,
    http: Client,
}

impl fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiClient")
            .field("base_url", &self.base_url)
            .field("token", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl ApiClient {
    /// Build a client bound to `base_url` and authenticating with `token`.
    ///
    /// Trims trailing slashes so callers can pass either `https://host` or
    /// `https://host/`. Rejects non-http(s) URLs and empty tokens loudly;
    /// silent fallback would mask a misconfigured client as "offline".
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> ApiClientResult<Self> {
        let base_url = normalize_base_url(base_url.into())?;
        let token = normalize_text_option(Some(token.into())).ok_or_else(|| {
            ApiClientError::InvalidConfiguration("DIRT_CLIENT_TOKEN must not be empty".into())
        })?;
        Ok(Self {
            base_url,
            token,
            http: Client::new(),
        })
    }

    /// Expose the normalized base URL for logging/diagnostics. The token is
    /// never exposed.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `POST /v1/notes/push`. Returns the per-id stamped timestamps.
    pub async fn push(&self, notes: &[PushNote]) -> ApiClientResult<PushResponse> {
        let url = format!("{}{PUSH_PATH}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&PushRequestBody { notes })
            .send()
            .await
            .map_err(|err| network_error(&err))?;
        parse_response::<PushResponse>(resp).await
    }

    /// `GET /v1/notes/pull`. `cursor=None` starts from the beginning;
    /// `limit=None` lets the server pick its default (see
    /// `dirt_api::turso::PULL_DEFAULT_LIMIT`).
    pub async fn pull(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> ApiClientResult<PullResponse> {
        let url = format!("{}{PULL_PATH}", self.base_url);
        let mut request = self.http.get(&url).bearer_auth(&self.token);
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor.to_string()));
        }
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        if !query.is_empty() {
            request = request.query(&query);
        }
        let resp = request.send().await.map_err(|err| network_error(&err))?;
        parse_response::<PullResponse>(resp).await
    }
}

// ---- Conversions ----

impl From<&Note> for PushNote {
    fn from(note: &Note) -> Self {
        Self {
            id: note.id.to_string(),
            content: note.content.clone(),
            created_at_ms: note.created_at,
            client_updated_at_ms: note.updated_at,
            deleted_at_ms: note.deleted_at,
        }
    }
}

impl TryFrom<PullNote> for Note {
    type Error = ApiClientError;

    fn try_from(remote: PullNote) -> Result<Self, Self::Error> {
        let id = NoteId::from_str(&remote.id).map_err(|err| {
            ApiClientError::Decode(format!("invalid note id '{}': {err}", remote.id))
        })?;
        Ok(Self {
            id,
            user_id: SOLO_USER_ID.to_string(),
            content: remote.content,
            created_at: remote.created_at_ms,
            updated_at: remote.client_updated_at_ms,
            server_updated_at: Some(remote.server_updated_at_ms),
            deleted_at: remote.deleted_at_ms,
        })
    }
}

// ---- Helpers ----

async fn parse_response<T: serde::de::DeserializeOwned>(resp: Response) -> ApiClientResult<T> {
    let status = resp.status();
    if status.is_success() {
        return resp
            .json::<T>()
            .await
            .map_err(|err| ApiClientError::Decode(err.to_string()));
    }

    let body = resp.text().await.unwrap_or_default();
    let (code, message) = serde_json::from_str::<ErrorEnvelope>(&body).map_or_else(
        |_| (String::new(), body.clone()),
        |env| (env.error.code, env.error.message),
    );

    Err(match status {
        StatusCode::UNAUTHORIZED => ApiClientError::Unauthorized(message),
        StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE => {
            ApiClientError::BadRequest { code, message }
        }
        StatusCode::SERVICE_UNAVAILABLE => ApiClientError::ServerUnavailable(message),
        other => ApiClientError::ServerError {
            status: other.as_u16(),
            message,
        },
    })
}

fn network_error(err: &reqwest::Error) -> ApiClientError {
    ApiClientError::Network(err.to_string())
}

fn normalize_base_url(raw: String) -> ApiClientResult<String> {
    let normalized = normalize_text_option(Some(raw)).ok_or_else(|| {
        ApiClientError::InvalidConfiguration("DIRT_API_BASE_URL must not be empty".into())
    })?;
    if !is_http_url(&normalized) {
        return Err(ApiClientError::InvalidConfiguration(
            "DIRT_API_BASE_URL must start with http:// or https://".into(),
        ));
    }
    Ok(normalized.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{bearer_token, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_TOKEN: &str = "test-client-token-0123456789abcdef";

    fn client_for(server: &MockServer) -> ApiClient {
        ApiClient::new(server.uri(), TEST_TOKEN).expect("client should build for mock server")
    }

    #[test]
    fn new_rejects_empty_base_url() {
        let err = ApiClient::new("", TEST_TOKEN).unwrap_err();
        assert!(matches!(err, ApiClientError::InvalidConfiguration(_)));
    }

    #[test]
    fn new_rejects_base_url_without_scheme() {
        let err = ApiClient::new("dirt-api.vercel.app", TEST_TOKEN).unwrap_err();
        assert!(matches!(err, ApiClientError::InvalidConfiguration(_)));
    }

    #[test]
    fn new_rejects_empty_token() {
        let err = ApiClient::new("https://example.com", "   ").unwrap_err();
        assert!(matches!(err, ApiClientError::InvalidConfiguration(_)));
    }

    #[test]
    fn new_trims_trailing_slash() {
        let client = ApiClient::new("https://example.com/", TEST_TOKEN).unwrap();
        assert_eq!(client.base_url(), "https://example.com");
    }

    #[test]
    fn debug_redacts_token() {
        let client = ApiClient::new("https://example.com", TEST_TOKEN).unwrap();
        let rendered = format!("{client:?}");
        assert!(!rendered.contains(TEST_TOKEN));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn pushnote_from_note_roundtrips_timestamps() {
        let mut note = Note::new("hello #world");
        note.created_at = 1;
        note.updated_at = 2;
        note.deleted_at = Some(3);
        let push: PushNote = (&note).into();
        assert_eq!(push.id, note.id.to_string());
        assert_eq!(push.content, "hello #world");
        assert_eq!(push.created_at_ms, 1);
        assert_eq!(push.client_updated_at_ms, 2);
        assert_eq!(push.deleted_at_ms, Some(3));
    }

    #[test]
    fn note_tryfrom_pullnote_preserves_server_updated_at() {
        let pull = PullNote {
            id: "01932aaa-0000-7000-8000-000000000abc".into(),
            content: "pulled".into(),
            created_at_ms: 10,
            client_updated_at_ms: 20,
            server_updated_at_ms: 30,
            deleted_at_ms: Some(25),
        };
        let note: Note = pull.try_into().expect("well-formed uuid parses");
        assert_eq!(note.user_id, SOLO_USER_ID);
        assert_eq!(note.created_at, 10);
        assert_eq!(note.updated_at, 20);
        assert_eq!(note.server_updated_at, Some(30));
        assert_eq!(note.deleted_at, Some(25));
        assert!(note.is_deleted());
    }

    #[test]
    fn note_tryfrom_pullnote_rejects_invalid_id() {
        let pull = PullNote {
            id: "not-a-uuid".into(),
            content: "x".into(),
            created_at_ms: 0,
            client_updated_at_ms: 0,
            server_updated_at_ms: 0,
            deleted_at_ms: None,
        };
        let err = Note::try_from(pull).unwrap_err();
        assert!(matches!(err, ApiClientError::Decode(_)));
    }

    #[tokio::test]
    async fn push_sends_bearer_and_body_and_decodes_response() {
        let server = MockServer::start().await;
        let note = PushNote {
            id: "01932aaa-0000-7000-8000-000000000abc".into(),
            content: "first".into(),
            created_at_ms: 1,
            client_updated_at_ms: 2,
            deleted_at_ms: None,
        };
        Mock::given(method("POST"))
            .and(path(PUSH_PATH))
            .and(bearer_token(TEST_TOKEN))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{
                    "id": note.id,
                    "applied": true,
                    "server_updated_at_ms": 99,
                }],
                "server_time_ms": 100,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let resp = client.push(std::slice::from_ref(&note)).await.unwrap();
        assert_eq!(resp.server_time_ms, 100);
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].id, note.id);
        assert!(resp.results[0].applied);
        assert_eq!(resp.results[0].server_updated_at_ms, 99);
    }

    #[tokio::test]
    async fn pull_forwards_cursor_and_limit_and_decodes_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(PULL_PATH))
            .and(bearer_token(TEST_TOKEN))
            .and(query_param("cursor", "abc"))
            .and(query_param("limit", "42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [{
                    "id": "01932aaa-0000-7000-8000-000000000abc",
                    "content": "hello",
                    "created_at_ms": 1,
                    "client_updated_at_ms": 2,
                    "server_updated_at_ms": 3,
                    "deleted_at_ms": null,
                }],
                "server_time_ms": 100,
                "has_more": true,
                "next_cursor": "def",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let resp = client.pull(Some("abc"), Some(42)).await.unwrap();
        assert_eq!(resp.notes.len(), 1);
        assert!(resp.has_more);
        assert_eq!(resp.next_cursor.as_deref(), Some("def"));
        assert_eq!(resp.notes[0].server_updated_at_ms, 3);
    }

    #[tokio::test]
    async fn pull_omits_query_params_when_none() {
        let server = MockServer::start().await;
        // Strict: if pull sent any query params, path() would still match
        // but we want to ensure the caller didn't accidentally pass empty
        // strings. wiremock doesn't expose "must-not-have-param" directly,
        // so we rely on the body parity + no matcher surprises.
        Mock::given(method("GET"))
            .and(path(PULL_PATH))
            .and(bearer_token(TEST_TOKEN))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 5,
                "has_more": false,
                "next_cursor": null,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let resp = client.pull(None, None).await.unwrap();
        assert!(resp.notes.is_empty());
        assert!(!resp.has_more);
        assert!(resp.next_cursor.is_none());
    }

    #[tokio::test]
    async fn unauthorized_maps_to_unauthorized_variant() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(PULL_PATH))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "UNAUTHORIZED",
                    "message": "missing Authorization header",
                    "cause": "No Authorization header present on the request.",
                    "fix": "Include 'Authorization: Bearer <DIRT_CLIENT_TOKEN>'."
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.pull(None, None).await.unwrap_err();
        assert!(
            matches!(err, ApiClientError::Unauthorized(ref msg) if msg.contains("missing Authorization"))
        );
    }

    #[tokio::test]
    async fn bad_request_preserves_server_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PUSH_PATH))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "code": "BATCH_TOO_LARGE",
                    "message": "batch has 501 notes; limit is 500",
                    "cause": "batch has 501 notes; limit is 500",
                    "fix": "Split the batch into groups of at most 500 notes and retry."
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.push(&[]).await.unwrap_err();
        match err {
            ApiClientError::BadRequest { code, message } => {
                assert_eq!(code, "BATCH_TOO_LARGE");
                assert!(message.contains("501"));
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn service_unavailable_maps_to_server_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(PULL_PATH))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({
                "error": {
                    "code": "TURSO_UNREACHABLE",
                    "message": "connection refused",
                    "cause": "connection refused",
                    "fix": "Retry shortly."
                }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.pull(None, None).await.unwrap_err();
        assert!(matches!(err, ApiClientError::ServerUnavailable(_)));
    }

    #[tokio::test]
    async fn unexpected_5xx_maps_to_server_error_with_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PUSH_PATH))
            .respond_with(ResponseTemplate::new(500).set_body_string("nginx went sideways"))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = client.push(&[]).await.unwrap_err();
        match err {
            ApiClientError::ServerError { status, message } => {
                assert_eq!(status, 500);
                assert!(message.contains("nginx"));
            }
            other => panic!("expected ServerError(500), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn network_error_when_server_unreachable() {
        // Port 1 is reserved (TCP multiplexer); connection is refused
        // instantly on every platform we care about, making this fast.
        let client = ApiClient::new("http://127.0.0.1:1", TEST_TOKEN).unwrap();
        let err = client.pull(None, None).await.unwrap_err();
        assert!(matches!(err, ApiClientError::Network(_)));
    }
}
