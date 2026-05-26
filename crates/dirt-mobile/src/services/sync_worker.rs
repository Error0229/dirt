//! Auto-sync background worker for the mobile shell.
//!
//! Mirrors `dirt-desktop/src/services/sync_worker.rs` — triggers,
//! debounce, backoff schedule, and the `SyncOutcome` taxonomy are
//! identical, including the silent-refresh path on 401. The only
//! mobile-specific tweak is the periodic cadence: 60 s instead of
//! desktop's 30 s, because mobile radio wake-ups carry a bigger battery
//! cost than a plugged-in desktop and the post-mutation kick already
//! covers user-driven activity.
//!
//! Triggers:
//!
//!   1. **Startup.** Once, immediately after the worker spawns. Pulls
//!      anything that landed remotely while the app was closed.
//!   2. **Periodic timer.** Every [`PERIODIC_INTERVAL`] regardless of
//!      activity.
//!   3. **Post-mutation kick.** Mutation sites call
//!      [`SyncWorkerHandle::trigger`] after a successful DB write; a
//!      [`POST_MUTATION_DEBOUNCE`] window collapses bursts.
//!
//! Auth refresh: on a 401 the worker calls
//! [`SessionApiClient::refresh`] to silently rotate the bearer. A
//! successful refresh swaps the inner client in place and the next
//! cycle uses the new token; a `SESSION_EXPIRED` refresh failure parks
//! the worker until an explicit kick (after the user logs back in via
//! the Account row in Settings).
//!
//! Shutdown: `SyncWorkerHandle::shutdown` is async — it flips the
//! shutdown flag, unparks the worker, and awaits the spawned tokio
//! task. The login / logout flows in `views::settings` rely on the
//! await so a newly-spawned worker can't race the previous one's
//! in-flight `sync_once`.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dirt_core::sync::api_client::ApiClientError;
use dirt_core::sync::engine::{SyncEngine, SyncEngineError};
use dirt_core::sync::scope_guard::{check_scope, ScopeCheckError};
use dirt_core::sync::session_client::{SessionApiClient, SessionRefreshError};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::data::MobileNoteStore;
use crate::state::SyncStatus;

/// Cadence of the periodic timer when no other triggers fire.
///
/// 60 s on mobile vs desktop's 30 s — see module docs.
pub const PERIODIC_INTERVAL: Duration = Duration::from_secs(60);

/// Time the worker waits after a mutation kick to coalesce additional
/// triggers before actually syncing.
pub const POST_MUTATION_DEBOUNCE: Duration = Duration::from_millis(1_500);

/// Backoff schedule applied after a failed sync cycle. Indexed by the
/// consecutive-failure count (capped at the last entry). Matches the
/// desktop schedule so a user with both apps installed sees the same
/// recovery curve on either side.
const BACKOFF_SCHEDULE: &[Duration] = &[
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
    Duration::from_secs(300),
];

/// Handle held by mutation sites and the login / logout flows.
///
/// The inner `JoinHandle` lives behind a `Mutex<Option<_>>` so the
/// *first* `shutdown().await` consumes it; subsequent calls see `None`
/// and return immediately. `shutdown_existing_worker` (in
/// `views::settings`) is idempotent across login / logout transitions
/// and relies on this.
#[derive(Clone)]
pub struct SyncWorkerHandle {
    notify: Arc<Notify>,
    shutdown: Arc<AtomicBool>,
    join: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl SyncWorkerHandle {
    /// Kick the worker. Cheap (just a `notify_one`). Safe to call from
    /// any tokio task; multiple calls before the worker debounces
    /// collapse into one extra sync cycle.
    pub fn trigger(&self) {
        self.notify.notify_one();
    }

    /// Signal the worker to exit and wait for any in-flight `sync_once`
    /// to complete before returning. The await is critical — without it
    /// a fresh worker spawned right after can race a still-executing
    /// push from the previous session.
    ///
    /// Idempotent: calling twice on the same handle returns immediately
    /// the second time (the `JoinHandle` is already gone).
    pub async fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Unpark a parked worker so it can observe the flag immediately
        // instead of waiting for the next mutation kick.
        self.notify.notify_one();
        let join = self
            .join
            .lock()
            .expect("sync worker join slot poisoned")
            .take();
        if let Some(handle) = join {
            // `JoinHandle::await` returns `Err` if the task panicked.
            // We ignore that — the worker only logs and the runtime
            // already surfaces the panic via tracing — but we still
            // wait so a panicked worker can't drag its `Arc<store>`
            // into a race with the newly spawned one.
            let _ = handle.await;
        }
    }
}

/// One status update emitted by the worker. The UI drains these into
/// dioxus signals — there's no shared state between the worker and the
/// UI besides this stream.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Coarse status — drives the indicator chip on the list view.
    Status(SyncStatus),
    /// Most recent error message, or `None` to clear after success.
    Issue(Option<String>),
    /// Unix-millis timestamp of the most recent successful sync.
    LastSync(i64),
}

/// Spawn a background sync worker bound to `(store, session)` and
/// return a handle the rest of the app can use to kick it. The worker
/// runs until the process exits or `SyncWorkerHandle::shutdown` is
/// called (the latter happens on sign-out).
pub fn spawn_sync_worker(
    store: Arc<MobileNoteStore>,
    session: Arc<SessionApiClient>,
    events: UnboundedSender<SyncEvent>,
) -> SyncWorkerHandle {
    let notify = Arc::new(Notify::new());
    let shutdown = Arc::new(AtomicBool::new(false));
    let notify_for_task = notify.clone();
    let shutdown_for_task = shutdown.clone();

    let task = tokio::spawn(async move {
        run_loop(store, session, notify_for_task, shutdown_for_task, events).await;
    });

    SyncWorkerHandle {
        notify,
        shutdown,
        join: Arc::new(Mutex::new(Some(task))),
    }
}

async fn run_loop(
    store: Arc<MobileNoteStore>,
    session: Arc<SessionApiClient>,
    notify: Arc<Notify>,
    shutdown: Arc<AtomicBool>,
    events: UnboundedSender<SyncEvent>,
) {
    if shutdown.load(Ordering::SeqCst) {
        return;
    }

    // Trigger 1: startup sync.
    let initial = sync_once(&store, &session, &events).await;
    let mut consecutive_failures = match initial {
        SyncOutcome::Ok => 0,
        SyncOutcome::Failed => 1,
        SyncOutcome::Permanent => {
            park_until_kick(&notify, &shutdown).await;
            if shutdown.load(Ordering::SeqCst) {
                return;
            }
            0
        }
    };

    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        let periodic_delay = if consecutive_failures == 0 {
            PERIODIC_INTERVAL
        } else {
            backoff_for(consecutive_failures)
        };

        let outcome = tokio::select! {
            // Trigger 2: periodic timer (or backoff timer when failing).
            () = tokio::time::sleep(periodic_delay) => {
                if shutdown.load(Ordering::SeqCst) { return; }
                sync_once(&store, &session, &events).await
            }
            // Trigger 3: post-mutation kick. Debounce so a burst of
            // edits coalesces into one sync.
            () = notify.notified() => {
                if shutdown.load(Ordering::SeqCst) { return; }
                tokio::time::sleep(POST_MUTATION_DEBOUNCE).await;
                if shutdown.load(Ordering::SeqCst) { return; }
                sync_once(&store, &session, &events).await
            }
        };

        consecutive_failures = match outcome {
            SyncOutcome::Ok => 0,
            SyncOutcome::Failed => consecutive_failures.saturating_add(1),
            SyncOutcome::Permanent => {
                park_until_kick(&notify, &shutdown).await;
                if shutdown.load(Ordering::SeqCst) {
                    return;
                }
                0
            }
        };
    }
}

async fn park_until_kick(notify: &Notify, shutdown: &AtomicBool) {
    if shutdown.load(Ordering::SeqCst) {
        return;
    }
    notify.notified().await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncOutcome {
    Ok,
    Failed,
    /// The error will keep failing under retry (terminal token state
    /// past silent-refresh) — surface to the user and stop scheduling
    /// automatic attempts.
    Permanent,
}

fn backoff_for(consecutive_failures: u32) -> Duration {
    // First failure (n=1) → BACKOFF_SCHEDULE[0]; clamps at the last entry.
    let idx = consecutive_failures
        .saturating_sub(1)
        .min(u32::try_from(BACKOFF_SCHEDULE.len() - 1).unwrap_or(u32::MAX)) as usize;
    BACKOFF_SCHEDULE[idx]
}

async fn sync_once(
    store: &MobileNoteStore,
    session: &SessionApiClient,
    events: &UnboundedSender<SyncEvent>,
) -> SyncOutcome {
    let _ = events.send(SyncEvent::Status(SyncStatus::Syncing));

    // Pre-sync scope check (issue #234). The keyring is shared across
    // CLI / desktop / mobile, so a peer client signing in as a
    // different account would otherwise let the worker push under the
    // wrong scope.
    match check_scope(store.user_id(), session) {
        Ok(()) => {}
        Err(ScopeCheckError::Mismatch {
            db_user,
            session_user,
        }) => {
            tracing::warn!(
                "Sync paused: DB belongs to {db_user} but session is for {session_user}"
            );
            let _ = events.send(SyncEvent::Status(SyncStatus::Error));
            let _ = events.send(SyncEvent::Issue(Some(format!(
                "This device opened the DB for {db_user} but the active session is {session_user}. \
                 Reopen the app (or sign in again) to switch accounts safely."
            ))));
            return SyncOutcome::Permanent;
        }
        Err(ScopeCheckError::SessionVanished) => {
            let _ = events.send(SyncEvent::Status(SyncStatus::Error));
            let _ = events.send(SyncEvent::Issue(Some(
                "The session was cleared from the keystore. Sign in again to resume sync.".into(),
            )));
            return SyncOutcome::Permanent;
        }
        Err(ScopeCheckError::Store(msg)) => {
            tracing::warn!("Pre-sync keystore read failed: {msg}");
            let _ = events.send(SyncEvent::Status(SyncStatus::Error));
            let _ = events.send(SyncEvent::Issue(Some(format!(
                "Could not read the session from the keystore: {msg}. Retrying shortly."
            ))));
            return SyncOutcome::Failed;
        }
    }

    let api = session.current();
    // `MobileNoteStore` derefs to the core `DatabaseService` the engine
    // expects, so rustc reborrows automatically.
    let engine = SyncEngine::new(store, &api, store.user_id());
    match engine.run_once().await {
        Ok(report) => {
            tracing::info!(
                "Sync complete — pulled {} (skipped {}), pushed {}",
                report.pulled_applied,
                report.pulled_skipped,
                report.pushed
            );
            let _ = events.send(SyncEvent::Status(SyncStatus::Synced));
            let _ = events.send(SyncEvent::Issue(None));
            let _ = events.send(SyncEvent::LastSync(chrono::Utc::now().timestamp_millis()));
            SyncOutcome::Ok
        }
        Err(err) => {
            if matches!(&err, SyncEngineError::Api(ApiClientError::Unauthorized(_))) {
                // Drop the snapshot Arc before refreshing — not load-
                // bearing (refresh takes the inner RwLock regardless),
                // just frees the old client before the next cycle
                // snapshots the refreshed one.
                drop(api);
                return handle_unauthorized(session, events).await;
            }
            tracing::error!("Sync failed: {err}");
            let _ = events.send(SyncEvent::Status(SyncStatus::Error));
            let _ = events.send(SyncEvent::Issue(Some(err.to_string())));
            SyncOutcome::Failed
        }
    }
}

/// Handle a 401 from the sync engine by attempting silent refresh.
///
/// Mirrors desktop. On success the worker returns `Failed` (not `Ok`)
/// so the loop immediately schedules another sync cycle — `Failed`
/// ramps backoff, but the first retry runs after only 5 s and uses the
/// fresh bearer, so the user-visible recovery is fast. On a permanent
/// refresh failure the keyring (here: `EncryptedPrefsTokenStore`) has
/// been cleared inside `refresh`; we surface a re-auth nudge through
/// `sync_issue` and park the worker.
async fn handle_unauthorized(
    session: &SessionApiClient,
    events: &UnboundedSender<SyncEvent>,
) -> SyncOutcome {
    match session.refresh().await {
        Ok(()) => {
            tracing::info!("Sync token refreshed silently after 401");
            // Don't surface a UI error — the user shouldn't notice the
            // background refresh. Mark Failed so we retry quickly with
            // the new client on the next cycle.
            SyncOutcome::Failed
        }
        Err(SessionRefreshError::SessionExpired(_) | SessionRefreshError::NoToken) => {
            let _ = events.send(SyncEvent::Status(SyncStatus::Error));
            let _ = events.send(SyncEvent::Issue(Some(
                "Your session has expired. Sign in again to resume sync.".into(),
            )));
            SyncOutcome::Permanent
        }
        Err(other) => {
            tracing::warn!("Token refresh failed transiently: {other}");
            let _ = events.send(SyncEvent::Status(SyncStatus::Error));
            let _ = events.send(SyncEvent::Issue(Some(format!(
                "Sync refresh failed: {other}"
            ))));
            SyncOutcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dirt_core::auth::{AuthClient, MemoryTokenStore, StoredToken, TokenStore};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const REFRESH_PATH: &str = "/v1/auth/refresh";

    fn seeded_store(token: &str) -> Arc<MemoryTokenStore> {
        Arc::new(MemoryTokenStore::with_initial(StoredToken {
            session_token: token.into(),
            session_id: "sid-old".into(),
            user_id: "uid-1".into(),
            email: "user@example.com".into(),
            expires_at_ms: 1,
        }))
    }

    fn session_for(server: &MockServer, store: Arc<dyn TokenStore>) -> SessionApiClient {
        let auth = AuthClient::new(server.uri()).expect("auth client should build for mock");
        SessionApiClient::from_store(server.uri(), auth, store)
            .expect("session client should build")
            .expect("seeded store should hydrate a session")
    }

    fn drain_events(rx: &mut tokio::sync::mpsc::UnboundedReceiver<SyncEvent>) -> Vec<SyncEvent> {
        let mut out = Vec::new();
        while let Ok(event) = rx.try_recv() {
            out.push(event);
        }
        out
    }

    #[test]
    fn unauthorized_classification_routes_through_refresh() {
        // Sanity check that the `is_permanent`-shaped logic from the
        // pre-Phase-2.7 worker is gone: a plain 401 from `sync_once` is
        // no longer terminal on its own; it gets routed to
        // `handle_unauthorized` which decides based on the refresh
        // response. The wiremock tests below cover the three branches.
        let _ = SyncEngineError::Api(ApiClientError::Unauthorized("dummy".into()));
    }

    #[test]
    fn backoff_walks_schedule_then_clamps() {
        assert_eq!(backoff_for(1), Duration::from_secs(5));
        assert_eq!(backoff_for(2), Duration::from_secs(15));
        assert_eq!(backoff_for(3), Duration::from_secs(60));
        assert_eq!(backoff_for(4), Duration::from_secs(300));
        // Past the end of the schedule it stays at the cap.
        assert_eq!(backoff_for(5), Duration::from_secs(300));
        assert_eq!(backoff_for(99), Duration::from_secs(300));
    }

    /// 401 from sync + a successful `/v1/auth/refresh` should return
    /// `Failed` (so the worker loops back quickly with the new bearer)
    /// and must NOT emit a UI-facing error event — the refresh is
    /// supposed to be invisible to the user.
    #[tokio::test(flavor = "current_thread")]
    async fn handle_unauthorized_returns_failed_on_successful_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REFRESH_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "session_token": "new-token",
                "session_id": "sid-new",
                "expires_at_ms": 9_999_999,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store_concrete = seeded_store("old-token");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = session_for(&server, store);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SyncEvent>();

        let outcome = handle_unauthorized(&session, &tx).await;
        assert_eq!(outcome, SyncOutcome::Failed);

        // Silent refresh: the worker should not push an Error onto the
        // status channel. (The caller already enqueued Syncing before
        // calling handle_unauthorized; we just confirm no extra noise.)
        assert!(
            drain_events(&mut rx).is_empty(),
            "successful refresh must not emit user-visible status events"
        );

        // And the new bearer must be persisted to the store so the next
        // sync cycle picks it up.
        assert_eq!(
            store_concrete.load().unwrap().unwrap().session_token,
            "new-token"
        );
    }

    /// 401 from sync + `SESSION_EXPIRED` on refresh is the terminal
    /// state — the worker must surface the "Sign in again" copy and
    /// return `Permanent` so the loop parks instead of hammering the
    /// server forever.
    #[tokio::test(flavor = "current_thread")]
    async fn handle_unauthorized_returns_permanent_on_session_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REFRESH_PATH))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "SESSION_EXPIRED",
                    "message": "session token is invalid or expired",
                    "cause": "session token is invalid or expired",
                    "fix": "Sign in again to obtain a fresh session token.",
                }
            })))
            .mount(&server)
            .await;

        let store_concrete = seeded_store("dead-token");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = session_for(&server, store);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SyncEvent>();

        let outcome = handle_unauthorized(&session, &tx).await;
        assert_eq!(outcome, SyncOutcome::Permanent);

        let events = drain_events(&mut rx);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SyncEvent::Status(SyncStatus::Error))),
            "expected Error status event, got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SyncEvent::Issue(Some(msg)) if msg.contains("Sign in again"))),
            "expected 'Sign in again' issue message, got {events:?}"
        );

        // SessionApiClient::refresh clears the store on SESSION_EXPIRED.
        assert!(store_concrete.load().unwrap().is_none());
    }

    /// 401 from sync + a transient 503 on refresh should classify as
    /// `Failed` (worker keeps retrying under backoff) and surface a
    /// distinct "Sync refresh failed" message — separate from the
    /// permanent "Sign in again" copy so a future log filter can tell
    /// them apart.
    #[tokio::test(flavor = "current_thread")]
    async fn handle_unauthorized_returns_failed_on_transient_refresh_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(REFRESH_PATH))
            .respond_with(ResponseTemplate::new(503).set_body_string("turso down"))
            .mount(&server)
            .await;

        let store_concrete = seeded_store("still-valid");
        let store: Arc<dyn TokenStore> = store_concrete.clone();
        let session = session_for(&server, store);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SyncEvent>();

        let outcome = handle_unauthorized(&session, &tx).await;
        assert_eq!(outcome, SyncOutcome::Failed);

        let events = drain_events(&mut rx);
        assert!(
            events.iter().any(
                |e| matches!(e, SyncEvent::Issue(Some(msg)) if msg.contains("Sync refresh failed"))
            ),
            "expected 'Sync refresh failed' issue message, got {events:?}"
        );

        // Transient failure must not clear the store — the credential
        // may still be valid; the worker will retry under backoff.
        assert!(store_concrete.load().unwrap().is_some());
    }
}
