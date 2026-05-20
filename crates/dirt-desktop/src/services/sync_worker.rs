//! Auto-sync background worker.
//!
//! There is no manual "Sync now" button — sync runs on three triggers:
//!
//!   1. **Startup.** Once, immediately after the worker spawns. Pulls
//!      anything that landed remotely while the app was closed.
//!   2. **Periodic timer.** Every [`PERIODIC_INTERVAL`] regardless of
//!      activity. Catches changes pushed by other devices that don't
//!      generate a local trigger.
//!   3. **Post-mutation kick.** Mutation sites call
//!      [`SyncWorkerHandle::trigger`] after a successful DB write. The
//!      worker debounces by [`POST_MUTATION_DEBOUNCE`] so a burst of
//!      keystrokes coalesces into a single sync rather than one per
//!      keypress.
//!
//! `tokio::sync::Notify` collapses multiple `notify_one()` calls into a
//! single `notified()` permit, so the debounce is naturally idempotent —
//! a hundred mutations during a 1.5 s window cause exactly one sync,
//! not a hundred queued ones.
//!
//! Status reaches the UI through an unbounded `mpsc` channel. Dioxus'
//! `UnsyncStorage`-backed `Signal`s aren't `Send`, so a consumer task
//! on the UI side drains the channel and writes the signals there;
//! the worker stays cleanly on the tokio side.
//!
//! Auth refresh: on a 401 the worker calls
//! [`SessionApiClient::refresh`] to silently rotate the bearer. A
//! successful refresh swaps the inner client in place and the next
//! cycle uses the new token; a `SESSION_EXPIRED` refresh failure parks
//! the worker until an explicit kick (after the user logs back in).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dirt_core::sync::api_client::ApiClientError;
use dirt_core::sync::engine::{SyncEngine, SyncEngineError};
use dirt_core::sync::session_client::{SessionApiClient, SessionRefreshError};
use dirt_core::SOLO_USER_ID;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

use crate::services::DatabaseService;
use crate::state::SyncStatus;

/// Cadence of the periodic timer when no other triggers fire.
pub const PERIODIC_INTERVAL: Duration = Duration::from_secs(30);
/// Time the worker waits after a mutation kick to coalesce additional
/// triggers before actually syncing.
pub const POST_MUTATION_DEBOUNCE: Duration = Duration::from_millis(1_500);

/// Backoff schedule applied after a failed sync cycle. Indexed by the
/// consecutive-failure count (capped at the last entry); the periodic
/// timer is replaced with these durations until the next success
/// resets the counter. Sized for unattended retries: don't hammer
/// the server when we've been told no, but recover quickly enough that
/// a transient blip doesn't leave the user offline for half an hour.
const BACKOFF_SCHEDULE: &[Duration] = &[
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
    Duration::from_secs(300),
];

/// Handle held by mutation sites so they can poke the worker without
/// taking a lock or reaching into the worker's internal state.
#[derive(Clone)]
pub struct SyncWorkerHandle {
    notify: Arc<Notify>,
    shutdown: Arc<AtomicBool>,
}

impl SyncWorkerHandle {
    /// Kick the worker. Cheap (just a `notify_one`). Safe to call from
    /// any tokio task; multiple calls before the worker debounces
    /// collapse into one extra sync cycle.
    pub fn trigger(&self) {
        self.notify.notify_one();
    }

    /// Signal the worker to exit at the next safe point. Used on
    /// sign-out so the worker doesn't keep holding `Arc`s to the
    /// database and session client after the user has revoked their
    /// token. Idempotent — calling twice is a no-op.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Unpark a parked worker so it can observe the flag immediately
        // instead of waiting for the next mutation kick.
        self.notify.notify_one();
    }
}

/// One status update emitted by the worker. The UI drains these into
/// dioxus signals — there's no shared state between the worker and the
/// UI besides this stream.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Coarse status — drives the indicator chip + Sync settings tab.
    Status(SyncStatus),
    /// Most recent error message, or `None` to clear after success.
    Issue(Option<String>),
    /// Unix-millis timestamp of the most recent successful sync.
    LastSync(i64),
}

/// Spawn a background sync worker bound to `(db, session)` and return a
/// handle the rest of the app can use to kick it. The worker runs until
/// the process exits or `SyncWorkerHandle::shutdown` is called (the
/// latter happens on sign-out).
pub fn spawn_sync_worker(
    db: Arc<DatabaseService>,
    session: Arc<SessionApiClient>,
    events: UnboundedSender<SyncEvent>,
) -> SyncWorkerHandle {
    let notify = Arc::new(Notify::new());
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = SyncWorkerHandle {
        notify: notify.clone(),
        shutdown: shutdown.clone(),
    };

    tokio::spawn(async move {
        run_loop(db, session, notify, shutdown, events).await;
    });

    handle
}

async fn run_loop(
    db: Arc<DatabaseService>,
    session: Arc<SessionApiClient>,
    notify: Arc<Notify>,
    shutdown: Arc<AtomicBool>,
    events: UnboundedSender<SyncEvent>,
) {
    if shutdown.load(Ordering::SeqCst) {
        return;
    }

    // Trigger 1: startup sync.
    let initial = sync_once(&db, &session, &events).await;
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
        // Periodic delay shrinks toward zero on success and stretches
        // along `BACKOFF_SCHEDULE` while we're failing, so a misconfigured
        // token doesn't slam the server every 30 s forever.
        let periodic_delay = if consecutive_failures == 0 {
            PERIODIC_INTERVAL
        } else {
            backoff_for(consecutive_failures)
        };

        let outcome = tokio::select! {
            // Trigger 2: periodic timer (or backoff timer when failing).
            () = tokio::time::sleep(periodic_delay) => {
                if shutdown.load(Ordering::SeqCst) { return; }
                sync_once(&db, &session, &events).await
            }
            // Trigger 3: post-mutation kick. Debounce so a burst of
            // edits coalesces into one sync.
            () = notify.notified() => {
                if shutdown.load(Ordering::SeqCst) { return; }
                tokio::time::sleep(POST_MUTATION_DEBOUNCE).await;
                if shutdown.load(Ordering::SeqCst) { return; }
                sync_once(&db, &session, &events).await
            }
        };

        consecutive_failures = match outcome {
            SyncOutcome::Ok => 0,
            SyncOutcome::Failed => consecutive_failures.saturating_add(1),
            SyncOutcome::Permanent => {
                // The error is not going to clear with retry (e.g.
                // server-side SESSION_EXPIRED past refresh). Park until
                // something kicks the worker explicitly — that gives the
                // login UI a chance to land a fresh token and notify the
                // worker, and avoids hammering the server forever.
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
    db: &DatabaseService,
    session: &SessionApiClient,
    events: &UnboundedSender<SyncEvent>,
) -> SyncOutcome {
    let _ = events.send(SyncEvent::Status(SyncStatus::Syncing));
    let api = session.current();
    // The desktop `DatabaseService` is a thin Deref wrapper around the
    // core service. Deref coercion does the right thing here — we
    // pass `&DatabaseService` and rustc reborrows it as the core type
    // `SyncEngine::new` expects.
    let engine = SyncEngine::new(db, &api, SOLO_USER_ID);
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
                // Release the snapshot Arc before `refresh` swaps the
                // inner client. Without this drop the worker would hold
                // a clone of the stale `ApiClient` across the refresh
                // (harmless, but noisy in logs).
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
/// On success the worker returns `Failed` (not `Ok`) so the loop
/// immediately schedules another sync cycle — `Failed` ramps backoff,
/// but the first retry runs after only 5 s and uses the fresh bearer,
/// so the user-visible recovery is fast. On a permanent refresh
/// failure the keyring has been cleared inside `refresh`; we surface a
/// re-auth nudge through `sync_issue` and park the worker.
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
}
