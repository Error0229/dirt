//! HTTP handlers for `/healthz`, `/v1/notes/push`, `/v1/notes/pull`.
//!
//! The bearer-token middleware runs before the push/pull handlers; here we
//! trust the request is authorized and every accepted note maps to the
//! solo-phase `SOLO_USER_ID`. Server timestamps are always stamped from the
//! handler's `now_ms()` — clients never drive `server_updated_at`.

use axum::Json;
use axum::extract::{Query, State};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use dirt_core::SOLO_USER_ID;
use dirt_core::models::NoteId;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::error::AppError;
use crate::turso::{PULL_DEFAULT_LIMIT, PULL_MAX_LIMIT, PUSH_BATCH_LIMIT, PushNote};

pub async fn healthz() -> &'static str {
    "ok"
}

// ---- POST /v1/notes/push ----

#[derive(Debug, Deserialize)]
pub struct PushRequest {
    pub notes: Vec<PushRequestNote>,
}

#[derive(Debug, Deserialize)]
pub struct PushRequestNote {
    pub id: String,
    pub content: String,
    pub created_at_ms: i64,
    pub client_updated_at_ms: i64,
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PushResponse {
    pub results: Vec<PushResult>,
    pub server_time_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct PushResult {
    pub id: String,
    pub applied: bool,
    pub server_updated_at_ms: i64,
}

pub async fn push_notes(
    State(state): State<AppState>,
    Json(body): Json<PushRequest>,
) -> Result<Json<PushResponse>, AppError> {
    if body.notes.len() > PUSH_BATCH_LIMIT {
        return Err(AppError::batch_too_large(format!(
            "batch has {} notes; limit is {PUSH_BATCH_LIMIT}",
            body.notes.len()
        )));
    }

    // Collapse intra-batch duplicates to the last occurrence so we apply
    // each id exactly once. The spec documents this as "last-occurrence
    // wins within a batch".
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for (idx, note) in body.notes.iter().enumerate() {
        seen.insert(note.id.clone(), idx);
    }
    let mut ordered: Vec<&PushRequestNote> = seen
        .values()
        .map(|&idx| &body.notes[idx])
        .collect();
    // Keep output order stable on id for predictable test assertions.
    ordered.sort_by(|a, b| a.id.cmp(&b.id));

    let server_now_ms = now_ms();
    let mut results = Vec::with_capacity(ordered.len());

    for note in ordered {
        let id = note
            .id
            .parse::<NoteId>()
            .map_err(|_| AppError::bad_request(format!("invalid note id: {}", note.id)))?;

        let push_note = PushNote {
            id: &id,
            content: note.content.as_str(),
            created_at_ms: note.created_at_ms,
            client_updated_at_ms: note.client_updated_at_ms,
            deleted_at_ms: note.deleted_at_ms,
        };

        let stamped = state
            .repo
            .upsert(SOLO_USER_ID, &push_note, server_now_ms)
            .await?;

        results.push(PushResult {
            id: note.id.clone(),
            applied: true,
            server_updated_at_ms: stamped,
        });
    }

    Ok(Json(PushResponse {
        results,
        server_time_ms: server_now_ms,
    }))
}

// ---- GET /v1/notes/pull?cursor=...&limit=... ----

#[derive(Debug, Deserialize)]
pub struct PullQuery {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct PullResponse {
    pub notes: Vec<PullNote>,
    pub server_time_ms: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PullNote {
    pub id: String,
    pub content: String,
    pub created_at_ms: i64,
    pub client_updated_at_ms: i64,
    pub server_updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

pub async fn pull_notes(
    State(state): State<AppState>,
    Query(params): Query<PullQuery>,
) -> Result<Json<PullResponse>, AppError> {
    let (cursor_sua, cursor_id) = decode_cursor(params.cursor.as_deref())?;
    let limit = params.limit.unwrap_or(PULL_DEFAULT_LIMIT).clamp(1, PULL_MAX_LIMIT);

    let rows = state
        .repo
        .pull_page(SOLO_USER_ID, cursor_sua, cursor_id.as_deref(), limit)
        .await?;

    let has_more = rows.len() == limit;
    let next_cursor = rows
        .last()
        .and_then(|note| note.server_updated_at.map(|sua| (sua, note.id.to_string())))
        .map(|(sua, id)| encode_cursor(sua, &id));

    let notes = rows
        .into_iter()
        .map(|note| PullNote {
            id: note.id.to_string(),
            content: note.content,
            created_at_ms: note.created_at,
            client_updated_at_ms: note.updated_at,
            server_updated_at_ms: note.server_updated_at.unwrap_or_default(),
            deleted_at_ms: note.deleted_at,
        })
        .collect();

    Ok(Json(PullResponse {
        notes,
        server_time_ms: now_ms(),
        has_more,
        next_cursor,
    }))
}

// ---- Cursor codec ----

#[derive(Serialize, Deserialize)]
struct CursorBody {
    sua: i64,
    id: String,
}

fn decode_cursor(raw: Option<&str>) -> Result<(i64, Option<String>), AppError> {
    let Some(raw) = raw else {
        return Ok((0, None));
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(raw.trim())
        .map_err(|_| AppError::bad_request("cursor is not valid base64url"))?;
    let body: CursorBody = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::bad_request("cursor payload is not valid JSON"))?;
    Ok((body.sua, Some(body.id)))
}

fn encode_cursor(sua: i64, id: &str) -> String {
    let json = serde_json::to_vec(&CursorBody {
        sua,
        id: id.to_string(),
    })
    .expect("CursorBody is always serializable");
    URL_SAFE_NO_PAD.encode(json)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrip() {
        let encoded = encode_cursor(1_700_000_000_000, "01932aaa-0000-7000-8000-000000000001");
        let (sua, id) = decode_cursor(Some(&encoded)).unwrap();
        assert_eq!(sua, 1_700_000_000_000);
        assert_eq!(id.as_deref(), Some("01932aaa-0000-7000-8000-000000000001"));
    }

    #[test]
    fn cursor_none_decodes_to_zero() {
        let (sua, id) = decode_cursor(None).unwrap();
        assert_eq!(sua, 0);
        assert!(id.is_none());
    }

    #[test]
    fn cursor_rejects_garbage() {
        assert!(decode_cursor(Some("not-base64!!!")).is_err());
    }
}
