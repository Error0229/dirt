//! `dirt sync` — push local mutations and pull remote changes once.
//!
//! Phase 2.8: the bearer is the magic-link session token persisted by
//! `dirt auth login` in the OS keychain. There is no longer a
//! `DIRT_CLIENT_TOKEN` fallback — if no session is stored the command
//! refuses to run and points the user at `dirt auth login`.
//!
//! Resolution order for the API base URL is unchanged from Phase 1:
//!   1. `DIRT_API_BASE_URL` env var (overrides everything for ad-hoc runs).
//!   2. Active CLI profile's `dirt_api_base_url`.
//!
//! Silent-refresh behaviour: on a 401 from the sync engine we call
//! [`SessionApiClient::refresh`] once and retry the cycle with the new
//! bearer. A second 401 (or a non-`Unauthorized` failure on the retry)
//! propagates as a normal error — the user re-runs `dirt sync` once
//! their session is valid again. This is the one-shot CLI counterpart
//! to the desktop worker's continuous-refresh loop in
//! `dirt-desktop/src/services/sync_worker.rs`.
//!
//! Phase 2.x: the local DB is per-user (see
//! `dirt_core::services::db_paths`), and the sync engine is keyed
//! by `db.user_id()` rather than the legacy `SOLO_USER_ID` sentinel.
//! A pre-sync `scope_guard::check_scope` refuses to push when the
//! keyring slot holds a bearer for a different user than the DB
//! belongs to — this closes the cross-client race where another
//! client signs in as a different account while this CLI invocation
//! is mid-flight.

use std::env;
use std::sync::Arc;

use dirt_core::auth::{AuthClient, KeyringTokenStore, TokenStore};
use dirt_core::sync::api_client::ApiClientError;
use dirt_core::sync::engine::{SyncEngine, SyncEngineError, SyncReport};
use dirt_core::sync::scope_guard::{check_scope, ScopeCheckError};
use dirt_core::sync::session_client::{SessionApiClient, SessionRefreshError};

use crate::commands::auth_cmd::{KEYRING_ACCOUNT, KEYRING_SERVICE};
use crate::commands::common::{open_database, DbScope};
use crate::config_profiles::{normalize_text_option, CliProfilesConfig};
use crate::error::CliError;

pub async fn run_sync(scope: &DbScope) -> Result<(), CliError> {
    let api_base_url = resolve_api_base_url()?;
    // Fixed `(service, "default")` keyring slot so a login from any
    // client (CLI / desktop / mobile) is visible here. The previous
    // profile-keyed account silently broke cross-client sharing under
    // `DIRT_PROFILE=foo` — see `KEYRING_ACCOUNT` doc-comment in
    // `commands::auth_cmd` for the full reasoning.
    let store: Arc<dyn TokenStore> =
        Arc::new(KeyringTokenStore::new(KEYRING_SERVICE, KEYRING_ACCOUNT));
    let session = build_session(api_base_url, store)?;
    let db = open_database(scope).await?;

    // Cross-client scope check: the keyring slot may have been
    // rotated to a different account by another process between the
    // moment `state.json` was last written and now. Refuse to push
    // rather than silently stamp this user's rows with a peer's
    // bearer.
    match check_scope(db.user_id(), &session) {
        Ok(()) => {}
        Err(ScopeCheckError::Mismatch {
            db_user,
            session_user,
        }) => {
            return Err(CliError::Auth(format!(
                "this CLI invocation opened the DB for user {db_user} but the keyring \
                 session belongs to {session_user}; another client signed in as a \
                 different account. Re-run `dirt sync` (or restart) to pick up the new \
                 active user, or run `dirt auth login` to sign back in as {db_user}."
            )));
        }
        Err(ScopeCheckError::SessionVanished) => {
            return Err(CliError::Auth(
                "the session token was cleared between login and sync; run `dirt auth login` again."
                    .into(),
            ));
        }
        Err(ScopeCheckError::Store(msg)) => {
            return Err(CliError::Config(format!(
                "could not verify session scope: {msg}; retry shortly"
            )));
        }
    }

    let report = run_session_sync(&db, &session).await?;
    print_report(&report);
    Ok(())
}

/// Build the refreshing session client from the keyring slot.
///
/// Factored out (rather than inlined in `run_sync`) so the tests can
/// drive the post-401 retry path against a wiremock-backed
/// `SessionApiClient` without booting the keyring.
fn build_session(
    api_base_url: String,
    store: Arc<dyn TokenStore>,
) -> Result<SessionApiClient, CliError> {
    let auth = AuthClient::new(&api_base_url).map_err(|err| {
        CliError::Config(format!(
            "invalid sync configuration: {err}; check DIRT_API_BASE_URL or `dirt config init`"
        ))
    })?;
    let session = SessionApiClient::from_store(api_base_url, auth, store).map_err(|err| {
        // `from_store` only fails in two ways: a Store error from the
        // keyring backend, or a Rebuild error from a malformed base URL.
        // Both are configuration-shaped: the user can fix one with
        // `dirt config init`, the other by unlocking the keyring.
        CliError::Config(format!("could not hydrate session: {err}"))
    })?;
    session.ok_or_else(|| {
        CliError::Auth(
            "not signed in; run `dirt auth login` to sign in, then re-run `dirt sync`".to_string(),
        )
    })
}

/// Run one sync cycle through `session`, with a single silent-refresh
/// retry on 401. Public-in-module so tests can drive the cycle directly.
async fn run_session_sync(
    db: &dirt_core::services::DatabaseService,
    session: &SessionApiClient,
) -> Result<SyncReport, CliError> {
    match sync_once(db, session).await {
        Ok(report) => Ok(report),
        Err(SyncEngineError::Api(ApiClientError::Unauthorized(_))) => {
            // Bearer was rejected mid-cycle. Try a silent refresh, then
            // exactly one retry — matches desktop's policy (a second 401
            // means refresh-without-actually-rotating-the-token, which
            // is a server bug and should surface, not loop).
            refresh_then_retry(db, session).await
        }
        Err(other) => Err(sync_engine_error_to_cli(&other)),
    }
}

async fn sync_once(
    db: &dirt_core::services::DatabaseService,
    session: &SessionApiClient,
) -> Result<SyncReport, SyncEngineError> {
    let api = session.current();
    let engine = SyncEngine::new(db, &api, db.user_id());
    engine.run_once().await
}

async fn refresh_then_retry(
    db: &dirt_core::services::DatabaseService,
    session: &SessionApiClient,
) -> Result<SyncReport, CliError> {
    match session.refresh().await {
        Ok(()) => {}
        Err(SessionRefreshError::SessionExpired(_) | SessionRefreshError::NoToken) => {
            return Err(CliError::Auth(
                "your session has expired; run `dirt auth login` to sign in again, then re-run `dirt sync`"
                    .to_string(),
            ));
        }
        Err(other) => {
            return Err(CliError::Config(format!(
                "session refresh failed during sync: {other}; retry shortly"
            )));
        }
    }

    sync_once(db, session)
        .await
        .map_err(|err| sync_engine_error_to_cli(&err))
}

fn resolve_api_base_url() -> Result<String, CliError> {
    if let Some(url) = normalize_text_option(env::var("DIRT_API_BASE_URL").ok()) {
        return Ok(url);
    }

    let config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(None);
    let profile = config
        .profile(&profile_name)
        .ok_or(CliError::SyncNotConfigured)?;
    profile
        .dirt_api_base_url()
        .ok_or(CliError::SyncNotConfigured)
}

fn sync_engine_error_to_cli(err: &SyncEngineError) -> CliError {
    match err {
        // A 401 reaching here means the retry-after-refresh path also
        // got 401 — the server rejected even the freshly rotated
        // bearer. Surface as auth-shaped so the user knows the next
        // step is re-login, not a transient retry.
        SyncEngineError::Api(ApiClientError::Unauthorized(msg)) => CliError::Auth(format!(
            "server rejected the refreshed session ({msg}); run `dirt auth login` again"
        )),
        other => CliError::Config(format!("sync failed: {other}")),
    }
}

fn print_report(report: &SyncReport) {
    println!(
        "Sync complete — pulled {} (skipped {}), pushed {}",
        report.pulled_applied, report.pulled_skipped, report.pushed
    );
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dirt_core::auth::{AuthClient, MemoryTokenStore, StoredToken, TokenStore};
    use dirt_core::sync::session_client::SessionApiClient;
    use serde_json::json;
    use wiremock::matchers::{bearer_token, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn seeded_store(token: &str) -> Arc<MemoryTokenStore> {
        Arc::new(MemoryTokenStore::with_initial(StoredToken {
            session_token: token.into(),
            session_id: "sid-1".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1_800_000_000_000_i64,
        }))
    }

    fn session_for(server: &MockServer, store: Arc<dyn TokenStore>) -> SessionApiClient {
        let auth = AuthClient::new(server.uri()).expect("auth client must build for mock");
        SessionApiClient::from_store(server.uri(), auth, store)
            .expect("from_store must succeed for mock")
            .expect("seeded store must hydrate a session")
    }

    async fn open_temp_db() -> dirt_core::services::DatabaseService {
        // In-memory libsql DB is enough — `SyncEngine` only reads
        // `sync_state` / `pending_sync` and writes the cursor, all of
        // which sit on top of the standard migration set that
        // `open_in_memory` runs internally.
        dirt_core::services::DatabaseService::open_in_memory()
            .await
            .expect("open in-memory db")
    }

    /// Empty pull + empty push must return Ok with zeroed counts when
    /// the session bearer is accepted. This is the happy-path smoke
    /// that proves the [`SessionApiClient`] → [`SyncEngine`] plumbing
    /// works with a session token (not a `DIRT_CLIENT_TOKEN`).
    #[tokio::test(flavor = "current_thread")]
    async fn run_session_sync_happy_path() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .and(bearer_token("tok-live"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 1_700_000_000_000_i64,
                "has_more": false,
                "next_cursor": null,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store: Arc<dyn TokenStore> = seeded_store("tok-live");
        let session = session_for(&server, store);
        let db = open_temp_db().await;

        let report = run_session_sync(&db, &session).await.unwrap();
        assert_eq!(report.pulled_applied, 0);
        assert_eq!(report.pushed, 0);
    }

    /// Single 401 → silent refresh → retry with new bearer → success.
    /// The user must NOT see the failure; the CLI prints the normal
    /// "Sync complete" line and exits 0. The refreshed token must be
    /// the one persisted in the store.
    #[tokio::test(flavor = "current_thread")]
    async fn run_session_sync_refreshes_on_401_and_retries() {
        let server = MockServer::start().await;
        // First pull with the stale bearer → 401.
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .and(bearer_token("tok-stale"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again."
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        // Refresh exchanges stale → fresh.
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .and(bearer_token("tok-stale"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "tok-fresh",
                "session_id": "sid-new",
                "expires_at_ms": 9_999_999_999_999_i64,
            })))
            .expect(1)
            .mount(&server)
            .await;
        // Retry pull with fresh bearer → empty page.
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .and(bearer_token("tok-fresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "notes": [],
                "server_time_ms": 1_700_000_000_000_i64,
                "has_more": false,
                "next_cursor": null,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store_concrete = seeded_store("tok-stale");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = session_for(&server, store);
        let db = open_temp_db().await;

        run_session_sync(&db, &session)
            .await
            .expect("retry after refresh must succeed");
        assert_eq!(
            store_concrete.load().unwrap().unwrap().session_token,
            "tok-fresh",
            "refresh must persist the new bearer"
        );
    }

    /// 401 on pull + `SESSION_EXPIRED` on `/v1/auth/refresh` is the
    /// terminal "the user has to log in again" path. The CLI must
    /// surface a [`CliError::Auth`] pointing at `dirt auth login`,
    /// and `SessionApiClient::refresh` must have cleared the keyring
    /// slot so the next run starts from a clean "Not signed in" state.
    #[tokio::test(flavor = "current_thread")]
    async fn run_session_sync_surfaces_session_expired_with_login_hint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again."
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again."
                }
            })))
            .mount(&server)
            .await;

        let store_concrete = seeded_store("tok-dead");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = session_for(&server, store);
        let db = open_temp_db().await;

        let err = run_session_sync(&db, &session)
            .await
            .expect_err("must surface auth error");
        match err {
            CliError::Auth(msg) => {
                assert!(msg.contains("session has expired"), "got {msg}");
                assert!(msg.contains("dirt auth login"), "got {msg}");
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
        assert!(
            store_concrete.load().unwrap().is_none(),
            "SessionApiClient::refresh must clear the keyring on SESSION_EXPIRED"
        );
    }

    /// 401 on pull + a transient 503 on refresh: the local credential
    /// is potentially still valid, so we must NOT clear the keyring and
    /// the CLI surfaces a `Config`-shaped (transient) error so the user
    /// knows to retry shortly — distinct from the "log in again" copy.
    #[tokio::test(flavor = "current_thread")]
    async fn run_session_sync_propagates_transient_refresh_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/notes/pull"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again."
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(503).set_body_string("turso down"))
            .mount(&server)
            .await;

        let store_concrete = seeded_store("tok-maybe-live");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = session_for(&server, store);
        let db = open_temp_db().await;

        let err = run_session_sync(&db, &session)
            .await
            .expect_err("must surface config error");
        match err {
            CliError::Config(msg) => {
                assert!(msg.contains("session refresh failed"), "got {msg}");
                assert!(msg.contains("retry shortly"), "got {msg}");
            }
            other => panic!("expected Config error, got {other:?}"),
        }
        assert!(
            store_concrete.load().unwrap().is_some(),
            "transient refresh failure must NOT clear the keyring"
        );
    }

    /// `build_session` returns Ok(None) → Auth error pointing at login
    /// when the keyring slot is empty. Exercises the "first-time user
    /// runs `dirt sync` without logging in" path.
    #[test]
    fn build_session_surfaces_not_signed_in_message() {
        let store: Arc<dyn TokenStore> = Arc::new(MemoryTokenStore::new());
        let err = build_session("https://example.invalid".into(), store).unwrap_err();
        match err {
            CliError::Auth(msg) => {
                assert!(msg.contains("not signed in"), "got {msg}");
                assert!(msg.contains("dirt auth login"), "got {msg}");
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
    }
}
