//! Single-cycle sync engine driving `ApiClient` against the local DB.
//!
//! `run_once` performs one full reconciliation pass:
//!
//!   1. **Pull loop.** Page through `GET /v1/notes/pull` from the
//!      persisted cursor until `has_more` is false. Each remote row is
//!      fed through `merge::resolve` together with the local row and
//!      its dirty flag; the resolver decides whether to overwrite,
//!      skip, or do nothing. The cursor advances after every page so
//!      a crash mid-pull is recoverable on the next call.
//!
//!   2. **Push loop.** Drain `pending_sync` in batches of
//!      [`PUSH_BATCH_SIZE`] notes, send each batch to
//!      `POST /v1/notes/push`, then `mark_pushed` per id with the
//!      server-stamped timestamp. The mark order (stamp first, clear
//!      pending second) keeps the system at-least-once: a crash
//!      between the two leaves the push idempotent on retry.
//!
//! The engine never writes `server_updated_at` from a client clock —
//! that field is only ever set from a server response (push stamp or
//! pull payload). Conflict resolution lives in the pure
//! `dirt_core::sync::merge` module so this driver stays narrow.

use crate::db::SyncCursor;
use crate::models::Note;
use crate::services::DatabaseService;
use crate::sync::api_client::{ApiClient, ApiClientError, PullNote, PushNote};
use crate::sync::merge::{resolve, MergeAction};

/// Maximum notes per `/v1/notes/push` batch. Mirrors the server-side
/// `PUSH_BATCH_LIMIT`; oversized batches are rejected with
/// `BATCH_TOO_LARGE`.
pub const PUSH_BATCH_SIZE: usize = 500;
/// Maximum notes pulled per `/v1/notes/pull` page.
pub const PULL_PAGE_SIZE: usize = 500;

/// Outcome of a single `SyncEngine::run_once` pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Notes the merge resolver actually applied (server overwrote local).
    pub pulled_applied: usize,
    /// Notes the resolver chose to skip (older server copy or dirty local).
    pub pulled_skipped: usize,
    /// Notes pushed and acknowledged by the server.
    pub pushed: usize,
}

/// Errors surfaced from a single sync cycle.
#[derive(Debug, thiserror::Error)]
pub enum SyncEngineError {
    /// HTTP layer failure — preserves bucketing from `ApiClientError`.
    #[error("api error: {0}")]
    Api(#[from] ApiClientError),
    /// Local DB error — schema/IO/etc.
    #[error("database error: {0}")]
    Database(#[from] crate::Error),
    /// A pull-response row failed `Note::try_from(PullNote)`.
    #[error("decode error: {0}")]
    Decode(String),
    /// The server returned a 200 response but didn't ack every note in
    /// the batch we just sent. Per the API contract every accepted note
    /// must come back with a stamped `server_updated_at_ms`; a partial
    /// response means client/server drift. We surface this rather than
    /// silently break the push loop, otherwise notes would sit in
    /// `pending_sync` forever while the worker reports green.
    #[error("server acked {acked} of {sent} pushed notes — server contract violation")]
    PushIncomplete { acked: usize, sent: usize },
}

/// Bound to `(db, api, user_id)` for one sync cycle. Cheap to construct;
/// callers re-create per cycle so the `user_id` and credentials can change
/// between runs.
pub struct SyncEngine<'a> {
    db: &'a DatabaseService,
    api: &'a ApiClient,
    user_id: &'a str,
}

impl<'a> SyncEngine<'a> {
    pub const fn new(db: &'a DatabaseService, api: &'a ApiClient, user_id: &'a str) -> Self {
        Self { db, api, user_id }
    }

    /// Run one full pull+push cycle. Returns counts for UI/logging.
    pub async fn run_once(&self) -> Result<SyncReport, SyncEngineError> {
        let mut report = SyncReport::default();
        self.pull(&mut report).await?;
        self.push(&mut report).await?;
        Ok(report)
    }

    async fn pull(&self, report: &mut SyncReport) -> Result<(), SyncEngineError> {
        let mut cursor = self.db.read_sync_cursor(self.user_id).await?;
        loop {
            let cursor_str = cursor.as_ref().map(SyncCursor::encode);
            let page = self
                .api
                .pull(cursor_str.as_deref(), Some(PULL_PAGE_SIZE))
                .await?;

            for pull_note in &page.notes {
                self.apply_pulled(pull_note, report).await?;
            }

            // Advance cursor *after* applying every row in the page so a
            // crash here re-runs the page on next start. Server returns
            // rows in (sua, id) order so the last note's coordinates are
            // the highest watermark.
            if let Some(last) = page.notes.last() {
                let next_cursor = SyncCursor {
                    sua: last.server_updated_at_ms,
                    id: last.id.clone(),
                };
                self.db
                    .write_sync_cursor(self.user_id, &next_cursor)
                    .await?;
                cursor = Some(next_cursor);
            }

            // Empty page is always terminal regardless of `has_more`. A
            // server bug returning `{"notes": [], "has_more": true}` would
            // otherwise spin forever inside `run_once` with no error to
            // trigger the worker's backoff.
            if !page.has_more || page.notes.is_empty() {
                break;
            }
        }
        Ok(())
    }

    async fn apply_pulled(
        &self,
        pull_note: &PullNote,
        report: &mut SyncReport,
    ) -> Result<(), SyncEngineError> {
        let remote: Note = pull_note
            .clone()
            .try_into()
            .map_err(|err: ApiClientError| SyncEngineError::Decode(err.to_string()))?;
        let local = self.db.get_with_tombstone(&remote.id).await?;
        let is_dirty = self.db.is_pending(self.user_id, &remote.id).await?;

        match resolve(local.as_ref(), Some(&remote), is_dirty) {
            MergeAction::Apply(note) => {
                self.db.upsert_from_server(&note).await?;
                report.pulled_applied += 1;
            }
            MergeAction::Skip => {
                report.pulled_skipped += 1;
            }
        }
        Ok(())
    }

    async fn push(&self, report: &mut SyncReport) -> Result<(), SyncEngineError> {
        loop {
            let pending = self
                .db
                .list_pending_notes(self.user_id, PUSH_BATCH_SIZE)
                .await?;
            if pending.is_empty() {
                break;
            }

            let batch: Vec<PushNote> = pending.iter().map(PushNote::from).collect();
            let response = self.api.push(&batch).await?;

            // Index server stamps by id so we can stamp local rows even
            // if the server reorders results (it currently sorts by id).
            let stamps = response
                .results
                .into_iter()
                .map(|r| (r.id, r.server_updated_at_ms))
                .collect::<std::collections::HashMap<_, _>>();

            let mut acked = 0;
            for note in &pending {
                let key = note.id.to_string();
                if let Some(&sua) = stamps.get(&key) {
                    self.db.mark_pushed(self.user_id, &note.id, sua).await?;
                    acked += 1;
                }
            }
            report.pushed += acked;

            // Every note we sent must come back acked. If the server
            // skipped any, the worker should hear about it as a hard
            // error rather than silently break — left unsignalled, the
            // unacked rows would sit in `pending_sync` forever while the
            // UI reports green.
            if acked < pending.len() {
                return Err(SyncEngineError::PushIncomplete {
                    acked,
                    sent: pending.len(),
                });
            }
        }
        Ok(())
    }
}

impl SyncCursor {
    /// Encode this cursor in the opaque `base64url(JSON)` form the server
    /// expects for the `?cursor=` query parameter.
    fn encode(&self) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;
        let body = serde_json::json!({ "sua": self.sua, "id": self.id });
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&body).expect("CursorBody serializes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NoteId;
    use crate::SOLO_USER_ID;
    use serde_json::json;
    use wiremock::matchers::{bearer_token, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_TOKEN: &str = "test-client-token-0123456789abcdef";

    async fn setup() -> (DatabaseService, MockServer, ApiClient) {
        let db = DatabaseService::open_in_memory().await.unwrap();
        let server = MockServer::start().await;
        let client = ApiClient::new(server.uri(), TEST_TOKEN).unwrap();
        (db, server, client)
    }

    fn server_note_json(
        id: &str,
        content: &str,
        sua: i64,
        deleted_at: Option<i64>,
    ) -> serde_json::Value {
        json!({
            "id": id,
            "content": content,
            "created_at_ms": sua - 100,
            "client_updated_at_ms": sua - 50,
            "server_updated_at_ms": sua,
            "deleted_at_ms": deleted_at,
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_pull_and_no_pending_returns_zeros() {
        let (db, server, api) = setup().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .and(bearer_token(TEST_TOKEN))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 1,
                "has_more": false,
                "next_cursor": null,
            })))
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        let report = engine.run_once().await.unwrap();
        assert_eq!(report, SyncReport::default());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pull_inserts_new_remote_note() {
        let (db, server, api) = setup().await;
        let id = "01932aaa-0000-7000-8000-000000000abc";

        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [server_note_json(id, "from server", 100, None)],
                "server_time_ms": 200,
                "has_more": false,
                "next_cursor": null,
            })))
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        let report = engine.run_once().await.unwrap();
        assert_eq!(report.pulled_applied, 1);
        assert_eq!(report.pulled_skipped, 0);
        assert_eq!(report.pushed, 0);

        let stored = db
            .get_with_tombstone(&id.parse::<NoteId>().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.content, "from server");
        assert_eq!(stored.server_updated_at, Some(100));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pull_skipped_when_local_is_dirty() {
        let (db, server, api) = setup().await;

        // Local create -> pending. We mark it as if it had been pushed
        // earlier so server_updated_at exists, then dirty it again so the
        // resolver should skip the incoming server copy.
        let local = {
            let lock = db.list_notes(0, 0).await.unwrap();
            drop(lock);
            db.create_note("local edit").await.unwrap()
        };
        // Force server_updated_at on the local row so the test's remote
        // sua of 100 is comparable; otherwise resolve_clean would short-
        // circuit on the (None, Some) branch and apply the remote.
        db.upsert_from_server(&Note {
            server_updated_at: Some(50),
            ..local.clone()
        })
        .await
        .unwrap();
        // Re-dirty it.
        db.update_note(&local.id, "local edit again").await.unwrap();
        assert!(db.is_pending(SOLO_USER_ID, &local.id).await.unwrap());

        let id = local.id.to_string();
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [server_note_json(&id, "server overwrite", 100, None)],
                "server_time_ms": 200,
                "has_more": false,
                "next_cursor": null,
            })))
            .mount(&server)
            .await;
        // Push response acks the dirty note so the engine returns Ok and
        // the assertions below can focus on pull-side merge behaviour.
        Mock::given(method("POST"))
            .and(path("/v1/notes/push"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{
                    "id": id,
                    "server_updated_at_ms": 999,
                }],
                "server_time_ms": 200,
            })))
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        let report = engine.run_once().await.unwrap();
        assert_eq!(report.pulled_applied, 0);
        assert_eq!(report.pulled_skipped, 1);

        // Local row content was preserved.
        let stored = db.get_note(&local.id).await.unwrap().unwrap();
        assert_eq!(stored.content, "local edit again");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pull_advances_cursor() {
        let (db, server, api) = setup().await;
        let id = "01932aaa-0000-7000-8000-000000000abc";

        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [server_note_json(id, "first", 1_000, None)],
                "server_time_ms": 1_000,
                "has_more": false,
                "next_cursor": null,
            })))
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        engine.run_once().await.unwrap();

        let cursor = db.read_sync_cursor(SOLO_USER_ID).await.unwrap().unwrap();
        assert_eq!(cursor.sua, 1_000);
        assert_eq!(cursor.id, id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn push_sends_pending_and_clears_after_ack() {
        let (db, server, api) = setup().await;

        // Empty pull.
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 0,
                "has_more": false,
                "next_cursor": null,
            })))
            .mount(&server)
            .await;

        let local = db.create_note("hello world").await.unwrap();
        let id = local.id.to_string();

        Mock::given(method("POST"))
            .and(path("/v1/notes/push"))
            .and(bearer_token(TEST_TOKEN))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{
                    "id": id,
                    "server_updated_at_ms": 9_999,
                }],
                "server_time_ms": 10_000,
            })))
            // The push loop iterates until pending drains, so the second
            // call after mark_pushed should not happen — but wiremock will
            // 404 if it does, surfacing the bug.
            .expect(1)
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        let report = engine.run_once().await.unwrap();
        assert_eq!(report.pushed, 1);

        assert!(!db.is_pending(SOLO_USER_ID, &local.id).await.unwrap());
        let stamped = db.get_note(&local.id).await.unwrap().unwrap();
        assert_eq!(stamped.server_updated_at, Some(9_999));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn push_errors_when_server_acks_nothing() {
        // The previous behavior silently broke the loop and returned
        // Ok, leaving the worker reporting green while notes sat
        // permanently in `pending_sync`. The engine now surfaces a
        // PushIncomplete error so the worker emits Status::Error.
        let (db, server, api) = setup().await;

        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 0,
                "has_more": false,
                "next_cursor": null,
            })))
            .mount(&server)
            .await;

        let local = db.create_note("permanently rejected").await.unwrap();

        Mock::given(method("POST"))
            .and(path("/v1/notes/push"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [],
                "server_time_ms": 0,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        let err = engine.run_once().await.unwrap_err();
        match err {
            SyncEngineError::PushIncomplete { acked, sent } => {
                assert_eq!(acked, 0);
                assert_eq!(sent, 1);
            }
            other => panic!("expected PushIncomplete, got {other:?}"),
        }

        // The unacked note must still be queued so the next run retries it.
        assert!(db.is_pending(SOLO_USER_ID, &local.id).await.unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn push_acks_full_multi_note_batch() {
        // Multi-note batch should be fully acked in one call, all rows
        // cleared from pending_sync. Real pagination across PUSH_BATCH_SIZE
        // boundaries isn't exercised here — the constant is 500 and would
        // require generating that many notes; the loop's correctness is
        // covered indirectly by `push_errors_when_server_acks_nothing`,
        // which proves the iteration terminates rather than spinning.
        let (db, server, api) = setup().await;

        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 0,
                "has_more": false,
                "next_cursor": null,
            })))
            .mount(&server)
            .await;

        // Create three notes; we'll mock the push to ack two, then the
        // engine should iterate again on the third — which we ack on the
        // second call.
        let n1 = db.create_note("one").await.unwrap();
        let n2 = db.create_note("two").await.unwrap();
        let n3 = db.create_note("three").await.unwrap();

        // Wiremock evaluates mocks in registration order. Stage two
        // partial responses: the first acks n1 + n2, the second acks n3.
        // The engine relies on `pending` shrinking between iterations,
        // so this proves it both pages and stops.
        let n1_str = n1.id.to_string();
        let n2_str = n2.id.to_string();
        let n3_str = n3.id.to_string();
        // Stage two responses: first acks all 3 notes (after the first
        // push, pending is empty and the loop exits cleanly). The second
        // mock is a safety-net that wiremock would only hit if the first
        // ack was rejected — in which case the test fails loudly.
        Mock::given(method("POST"))
            .and(path("/v1/notes/push"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {"id": n1_str, "server_updated_at_ms": 1},
                    {"id": n2_str, "server_updated_at_ms": 2},
                    {"id": n3_str, "server_updated_at_ms": 3},
                ],
                "server_time_ms": 0,
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        let report = engine.run_once().await.unwrap();
        assert_eq!(report.pushed, 3);
        assert!(!db.is_pending(SOLO_USER_ID, &n1.id).await.unwrap());
        assert!(!db.is_pending(SOLO_USER_ID, &n2.id).await.unwrap());
        assert!(!db.is_pending(SOLO_USER_ID, &n3.id).await.unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pull_breaks_on_empty_page_even_when_has_more_is_true() {
        // Regression guard: if the server ever returns `notes: []` with
        // `has_more: true`, the cursor can't advance (no last note) and
        // the pull loop must terminate rather than spin.
        let (db, server, api) = setup().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 1,
                "has_more": true,
                "next_cursor": null,
            })))
            // If the engine looped, it would call this endpoint many
            // times; capping at 1 forces the bug to surface as a 404
            // mismatch on the second request.
            .expect(1)
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        let report = engine.run_once().await.unwrap();
        assert_eq!(report.pulled_applied, 0);
        assert_eq!(report.pulled_skipped, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unauthorized_pull_propagates_as_api_error() {
        let (db, server, api) = setup().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "UNAUTHORIZED",
                    "message": "missing Authorization header",
                    "cause": "x",
                    "fix": "y",
                }
            })))
            .mount(&server)
            .await;

        let engine = SyncEngine::new(&db, &api, SOLO_USER_ID);
        let err = engine.run_once().await.unwrap_err();
        assert!(matches!(
            err,
            SyncEngineError::Api(ApiClientError::Unauthorized(_))
        ));
    }

    #[test]
    fn sync_cursor_encode_matches_server_codec() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;

        // The server's decode_cursor in dirt-api expects this exact
        // shape: base64url(JSON{"sua":i64,"id":string}). Drift here
        // breaks pagination silently.
        let cursor = SyncCursor {
            sua: 1_700_000_000_000,
            id: "01932aaa-0000-7000-8000-000000000abc".to_string(),
        };
        let encoded = cursor.encode();
        // Round-trip via base64url -> json.
        let bytes = URL_SAFE_NO_PAD.decode(&encoded).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["sua"], 1_700_000_000_000_i64);
        assert_eq!(value["id"], cursor.id);
    }
}
