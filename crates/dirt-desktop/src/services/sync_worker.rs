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

use std::sync::Arc;
use std::time::Duration;

use dirt_core::SOLO_USER_ID;
use dirt_core::sync::api_client::ApiClient;
use dirt_core::sync::engine::SyncEngine;
use tokio::sync::Notify;
use tokio::sync::mpsc::UnboundedSender;

use crate::services::DatabaseService;
use crate::state::SyncStatus;

/// Cadence of the periodic timer when no other triggers fire.
pub const PERIODIC_INTERVAL: Duration = Duration::from_secs(30);
/// Time the worker waits after a mutation kick to coalesce additional
/// triggers before actually syncing.
pub const POST_MUTATION_DEBOUNCE: Duration = Duration::from_millis(1_500);

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
    /// Coarse status — drives the indicator chip + Sync settings tab.
    Status(SyncStatus),
    /// Most recent error message, or `None` to clear after success.
    Issue(Option<String>),
    /// Unix-millis timestamp of the most recent successful sync.
    LastSync(i64),
}

/// Spawn a background sync worker bound to `(db, api)` and return a
/// handle the rest of the app can use to kick it. The worker runs
/// until the process exits — no Drop-based cancellation, no shutdown
/// signal: a desktop process getting torn down means the OS reaps the
/// task. If we ever need cleaner shutdown semantics that's the place
/// to add a `CancellationToken`.
pub fn spawn_sync_worker(
    db: Arc<DatabaseService>,
    api: Arc<ApiClient>,
    events: UnboundedSender<SyncEvent>,
) -> SyncWorkerHandle {
    let notify = Arc::new(Notify::new());
    let handle = SyncWorkerHandle {
        notify: notify.clone(),
    };

    tokio::spawn(async move {
        run_loop(db, api, notify, events).await;
    });

    handle
}

async fn run_loop(
    db: Arc<DatabaseService>,
    api: Arc<ApiClient>,
    notify: Arc<Notify>,
    events: UnboundedSender<SyncEvent>,
) {
    // Trigger 1: startup sync.
    sync_once(&db, &api, &events).await;

    loop {
        tokio::select! {
            // Trigger 2: periodic timer.
            () = tokio::time::sleep(PERIODIC_INTERVAL) => {
                sync_once(&db, &api, &events).await;
            }
            // Trigger 3: post-mutation kick. Debounce so a burst of
            // edits coalesces into one sync.
            () = notify.notified() => {
                tokio::time::sleep(POST_MUTATION_DEBOUNCE).await;
                sync_once(&db, &api, &events).await;
            }
        }
    }
}

async fn sync_once(
    db: &DatabaseService,
    api: &ApiClient,
    events: &UnboundedSender<SyncEvent>,
) {
    let _ = events.send(SyncEvent::Status(SyncStatus::Syncing));
    // The desktop `DatabaseService` is a thin Deref wrapper around the
    // core service. Deref coercion does the right thing here — we
    // pass `&DatabaseService` and rustc reborrows it as the core type
    // `SyncEngine::new` expects.
    let engine = SyncEngine::new(db, api, SOLO_USER_ID);
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
            let _ = events.send(SyncEvent::LastSync(
                chrono::Utc::now().timestamp_millis(),
            ));
        }
        Err(err) => {
            tracing::error!("Sync failed: {err}");
            let _ = events.send(SyncEvent::Status(SyncStatus::Error));
            let _ = events.send(SyncEvent::Issue(Some(err.to_string())));
        }
    }
}
