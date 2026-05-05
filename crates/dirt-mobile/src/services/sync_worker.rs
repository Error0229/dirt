//! Auto-sync background worker for the mobile shell.
//!
//! Mirrors `dirt-desktop/src/services/sync_worker.rs` — the triggers,
//! the debounce, the backoff schedule, and the `SyncOutcome` taxonomy
//! are identical. The only differences live at the edges:
//!
//!   * the worker is bound to `MobileNoteStore` (which derefs to the
//!     core `DatabaseService` the engine wants), and
//!   * the periodic cadence is loosened a little, since mobile devices
//!     pay a bigger battery cost for each wake-up than a plugged-in
//!     desktop.
//!
//! Sync runs on three triggers:
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
//! single `notified()` permit, so the debounce is naturally idempotent.
//!
//! Status reaches the UI through an unbounded `mpsc` channel. Dioxus
//! signals aren't `Send`, so a consumer task on the UI side drains the
//! channel and writes the signals there; the worker stays cleanly on
//! the tokio side.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::sync::Arc;
use std::time::Duration;

use dirt_core::sync::api_client::{ApiClient, ApiClientError};
use dirt_core::sync::engine::{SyncEngine, SyncEngineError};
use dirt_core::SOLO_USER_ID;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

use crate::data::MobileNoteStore;
use crate::state::SyncStatus;

/// Cadence of the periodic timer when no other triggers fire.
///
/// 60 s on mobile vs desktop's 30 s — mobile radio wake-ups are
/// expensive, and the post-mutation kick already covers user-driven
/// activity. Cross-device propagation latency is bounded by this for
/// the idle case.
pub const PERIODIC_INTERVAL: Duration = Duration::from_secs(60);

/// Time the worker waits after a mutation kick to coalesce additional
/// triggers before actually syncing.
pub const POST_MUTATION_DEBOUNCE: Duration = Duration::from_millis(1_500);

/// Backoff schedule applied after a failed sync cycle. Indexed by the
/// consecutive-failure count (capped at the last entry); the periodic
/// timer is replaced with these durations until the next success
/// resets the counter.
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
}

impl SyncWorkerHandle {
    /// Kick the worker. Cheap (just a `notify_one`). Safe to call from
    /// any tokio task; multiple calls before the worker debounces
    /// collapse into one extra sync cycle.
    pub fn trigger(&self) {
        self.notify.notify_one();
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

/// Spawn a background sync worker bound to `(store, api)` and return a
/// handle the rest of the app can use to kick it. The worker runs
/// until the process exits.
pub fn spawn_sync_worker(
    store: Arc<MobileNoteStore>,
    api: Arc<ApiClient>,
    events: UnboundedSender<SyncEvent>,
) -> SyncWorkerHandle {
    let notify = Arc::new(Notify::new());
    let handle = SyncWorkerHandle {
        notify: notify.clone(),
    };

    tokio::spawn(async move {
        run_loop(store, api, notify, events).await;
    });

    handle
}

async fn run_loop(
    store: Arc<MobileNoteStore>,
    api: Arc<ApiClient>,
    notify: Arc<Notify>,
    events: UnboundedSender<SyncEvent>,
) {
    // Trigger 1: startup sync.
    let initial = sync_once(&store, &api, &events).await;
    let mut consecutive_failures = match initial {
        SyncOutcome::Ok => 0,
        SyncOutcome::Failed => 1,
        SyncOutcome::Permanent => {
            park_until_kick(&notify).await;
            0
        }
    };

    loop {
        let periodic_delay = if consecutive_failures == 0 {
            PERIODIC_INTERVAL
        } else {
            backoff_for(consecutive_failures)
        };

        let outcome = tokio::select! {
            // Trigger 2: periodic timer (or backoff timer when failing).
            () = tokio::time::sleep(periodic_delay) => {
                sync_once(&store, &api, &events).await
            }
            // Trigger 3: post-mutation kick. Debounce so a burst of
            // edits coalesces into one sync.
            () = notify.notified() => {
                tokio::time::sleep(POST_MUTATION_DEBOUNCE).await;
                sync_once(&store, &api, &events).await
            }
        };

        consecutive_failures = match outcome {
            SyncOutcome::Ok => 0,
            SyncOutcome::Failed => consecutive_failures.saturating_add(1),
            SyncOutcome::Permanent => {
                park_until_kick(&notify).await;
                0
            }
        };
    }
}

async fn park_until_kick(notify: &Notify) {
    notify.notified().await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncOutcome {
    Ok,
    Failed,
    /// The error will keep failing under retry — surface to the user
    /// and stop scheduling automatic attempts. Currently triggered by
    /// `Unauthorized`.
    Permanent,
}

fn backoff_for(consecutive_failures: u32) -> Duration {
    let idx = consecutive_failures
        .saturating_sub(1)
        .min(u32::try_from(BACKOFF_SCHEDULE.len() - 1).unwrap_or(u32::MAX)) as usize;
    BACKOFF_SCHEDULE[idx]
}

async fn sync_once(
    store: &MobileNoteStore,
    api: &ApiClient,
    events: &UnboundedSender<SyncEvent>,
) -> SyncOutcome {
    let _ = events.send(SyncEvent::Status(SyncStatus::Syncing));
    let engine = SyncEngine::new(store, api, SOLO_USER_ID);
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
            tracing::error!("Sync failed: {err}");
            let _ = events.send(SyncEvent::Status(SyncStatus::Error));
            let _ = events.send(SyncEvent::Issue(Some(err.to_string())));
            if is_permanent(&err) {
                SyncOutcome::Permanent
            } else {
                SyncOutcome::Failed
            }
        }
    }
}

const fn is_permanent(err: &SyncEngineError) -> bool {
    matches!(err, SyncEngineError::Api(ApiClientError::Unauthorized(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_classifies_as_permanent() {
        let err = SyncEngineError::Api(ApiClientError::Unauthorized("bad token".into()));
        assert!(is_permanent(&err));
    }

    #[test]
    fn transient_errors_stay_failed() {
        let cases = [
            SyncEngineError::Api(ApiClientError::Network("timeout".into())),
            SyncEngineError::Api(ApiClientError::ServerUnavailable("turso down".into())),
            SyncEngineError::Api(ApiClientError::ServerError {
                status: 500,
                message: "boom".into(),
            }),
            SyncEngineError::PushIncomplete { acked: 0, sent: 1 },
            SyncEngineError::Decode("contract drift".into()),
        ];
        for err in cases {
            assert!(!is_permanent(&err), "{err} should not be permanent");
        }
    }

    #[test]
    fn backoff_walks_schedule_then_clamps() {
        assert_eq!(backoff_for(1), Duration::from_secs(5));
        assert_eq!(backoff_for(2), Duration::from_secs(15));
        assert_eq!(backoff_for(3), Duration::from_secs(60));
        assert_eq!(backoff_for(4), Duration::from_secs(300));
        assert_eq!(backoff_for(5), Duration::from_secs(300));
        assert_eq!(backoff_for(99), Duration::from_secs(300));
    }
}
