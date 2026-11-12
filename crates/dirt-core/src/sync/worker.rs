//! The background worker that decides when to sync.
//!
//! Sync is driven by events. There is no timer that fires while nothing
//! happens (design section 8.3). The worker runs a cycle:
//!
//! - when the app starts;
//! - when the person returns to the app, unless it synced a moment ago;
//! - shortly after a local change, once changes stop arriving;
//! - when the person presses the sync button;
//! - again, with growing pauses, only while a cycle has failed and notes are
//!   still waiting to be sent.
//!
//! An app that is open and untouched does not talk to the server.

use std::sync::Arc;
use std::time::Duration;

use dirt_proto::api::ErrorClass;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::engine::{SyncEngine, SyncError};
use crate::api::ApiError;
use crate::Store;

/// Something that happened which may call for a sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The app started, or the person just signed in.
    Start,
    /// The person came back to the app: its window gained focus, or it
    /// returned to the foreground.
    Returned,
    /// A note was created, edited or deleted here.
    Changed,
    /// The person pressed the sync button.
    Manual,
}

/// Why the last cycle did not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncFailure {
    /// What the app should do about it.
    pub class: ErrorClass,
    pub message: String,
}

/// What an interface shows about sync.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncStatus {
    /// A cycle is running now.
    pub syncing: bool,
    /// When a cycle last completed, as Unix milliseconds.
    pub last_synced_ms: Option<i64>,
    /// Notes waiting to be sent.
    pub unsent: usize,
    /// Notes that could not be synced. Each has a reason in the store.
    pub problems: usize,
    /// Why the last cycle failed, if it did.
    pub failure: Option<SyncFailure>,
    /// How many documents have been pulled and merged since the worker
    /// started, counting those merged by a cycle that then failed. A change
    /// in this number means the notes on screen may be out of date.
    pub pulled: u64,
    /// Devices waiting for this one to approve them, as of the last sync.
    pub pending_devices: u32,
    /// When a reset that a waiting device asked for takes effect, if one is
    /// pending. An approved device should refuse it unless the person
    /// expected it.
    pub reset_effective_at_ms: Option<i64>,
    /// The version of the account's key that notes are written under, as
    /// of the last sync. Zero before the first.
    pub key_version: i32,
    /// Notes still stored under an older key, as of the last sync. While
    /// there are any, a change of the account's key is not complete.
    pub old_key_notes: u64,
}

/// How long the worker waits in each situation.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// The quiet time after a local change before syncing.
    pub debounce: Duration,
    /// The longest a stream of changes can put a sync off.
    pub debounce_limit: Duration,
    /// Returning to the app does not sync if a cycle ran this recently.
    pub return_gap: Duration,
    /// The pause before the first retry. Each later pause is twice as long.
    pub first_retry: Duration,
    /// The longest pause between retries.
    pub longest_retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            debounce: Duration::from_secs(2),
            debounce_limit: Duration::from_secs(15),
            return_gap: Duration::from_secs(30),
            first_retry: Duration::from_secs(5),
            longest_retry: Duration::from_secs(300),
        }
    }
}

/// A running worker. Dropping it stops the worker.
#[derive(Debug)]
pub struct SyncWorker {
    triggers: mpsc::UnboundedSender<Trigger>,
    task: JoinHandle<()>,
}

impl SyncWorker {
    /// Starts a worker on the current Tokio runtime. It publishes what it
    /// does to `status`.
    #[must_use]
    pub fn spawn(
        engine: Arc<SyncEngine>,
        store: Store,
        status: Arc<watch::Sender<SyncStatus>>,
        timing: Timing,
    ) -> Self {
        let (triggers, inbox) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(engine, store, status, timing, inbox));
        Self { triggers, task }
    }

    /// Tells the worker that something happened. Never blocks.
    pub fn trigger(&self, trigger: Trigger) {
        // The worker only stops when this handle is dropped.
        let _ = self.triggers.send(trigger);
    }
}

impl Drop for SyncWorker {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The pause the server asked for, if the failure carries one.
fn server_pause(error: &SyncError) -> Option<Duration> {
    match error {
        SyncError::Api(ApiError::Server {
            retry_after_secs: Some(seconds),
            ..
        }) => Some(Duration::from_secs(u64::from(*seconds))),
        _ => None,
    }
}

/// What the worker is waiting for besides triggers.
#[derive(Default)]
struct Timers {
    /// When to sync for the changes that have arrived.
    change_due: Option<Instant>,
    /// The latest moment those changes may wait until.
    change_limit: Option<Instant>,
    /// When to try again after a failure, and the pause that led there.
    retry: Option<(Instant, Duration)>,
}

impl Timers {
    fn next(&self) -> Option<Instant> {
        match (self.change_due, self.retry.map(|(at, _)| at)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn note_change(&mut self, timing: &Timing) {
        let now = Instant::now();
        let limit = *self
            .change_limit
            .get_or_insert_with(|| now + timing.debounce_limit);
        self.change_due = Some((now + timing.debounce).min(limit));
    }

    fn schedule_retry(&mut self, timing: &Timing, at_least: Option<Duration>) {
        let pause = self.retry.map_or(timing.first_retry, |(_, last)| {
            (last * 2).min(timing.longest_retry)
        });
        let wait = at_least.map_or(pause, |asked| asked.max(pause));
        self.retry = Some((Instant::now() + wait, pause));
    }
}

async fn run(
    engine: Arc<SyncEngine>,
    store: Store,
    status: Arc<watch::Sender<SyncStatus>>,
    timing: Timing,
    mut inbox: mpsc::UnboundedReceiver<Trigger>,
) {
    let mut timers = Timers::default();
    let mut last_cycle: Option<Instant> = None;
    // Set when the server says this device must sign in again or the app
    // must be updated. Trying again by itself would achieve nothing, so
    // from then on only the person can start a cycle.
    let mut stopped = false;

    refresh_counts(&store, &status).await;

    loop {
        let due = timers.next();
        let trigger = tokio::select! {
            trigger = inbox.recv() => match trigger {
                Some(trigger) => Some(trigger),
                None => return,
            },
            () = async {
                match due {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => None,
        };

        let run_now = match trigger {
            Some(Trigger::Start | Trigger::Manual) => true,
            Some(Trigger::Returned) => {
                !stopped && last_cycle.is_none_or(|at| at.elapsed() >= timing.return_gap)
            }
            Some(Trigger::Changed) => {
                refresh_counts(&store, &status).await;
                if !stopped {
                    timers.note_change(&timing);
                }
                false
            }
            // A timer came due.
            None => {
                if stopped {
                    timers = Timers::default();
                }
                !stopped
            }
        };
        if !run_now {
            continue;
        }

        timers.change_due = None;
        timers.change_limit = None;
        status.send_modify(|status| status.syncing = true);
        let outcome = engine.run_once().await;
        last_cycle = Some(Instant::now());

        let failure = match &outcome {
            Ok(report) => {
                tracing::info!(
                    "synced: {} received, {} sent, {} waiting",
                    report.pulled,
                    report.pushed,
                    report.unsent
                );
                stopped = false;
                if report.unsent > 0 {
                    // Other devices kept changing the same notes. Try again
                    // soon, without calling it a failure.
                    timers.schedule_retry(&timing, None);
                } else {
                    timers.retry = None;
                }
                None
            }
            Err(error) => {
                let class = error.class();
                tracing::warn!("sync did not complete: {error}");
                if class == ErrorClass::TryAgainLater {
                    // Retry only while something is waiting to be sent.
                    // With nothing to send, the next event is soon enough.
                    if store.unsent_count().await.unwrap_or(0) > 0 {
                        timers.schedule_retry(&timing, server_pause(error));
                    } else {
                        timers.retry = None;
                    }
                } else {
                    stopped = true;
                    timers.retry = None;
                }
                Some(SyncFailure {
                    class,
                    message: error.to_string(),
                })
            }
        };

        let unsent = store.unsent_count().await.unwrap_or(0);
        let problems = store.note_problems().await.map_or(0, |list| list.len());
        let last_synced_ms = store.last_synced_ms().await.unwrap_or(None);
        // Read from the engine and not from the cycle's report: a cycle that
        // failed has no report, but may have merged documents first.
        let pulled = engine.merged_total();
        let attention = outcome.as_ref().ok().map(|report| {
            (
                report.pending_devices,
                report.reset_effective_at_ms,
                report.key_version,
                report.old_key_notes,
            )
        });
        status.send_modify(|status| {
            // A cycle that failed says nothing new about these.
            if let Some((pending_devices, reset_effective_at_ms, key_version, old_key_notes)) =
                attention
            {
                status.pending_devices = pending_devices;
                status.reset_effective_at_ms = reset_effective_at_ms;
                status.key_version = key_version;
                status.old_key_notes = old_key_notes;
            }
            status.syncing = false;
            status.last_synced_ms = last_synced_ms;
            status.unsent = unsent;
            status.problems = problems;
            status.failure = failure;
            status.pulled = pulled;
        });
    }
}

/// Publishes how many notes are waiting, without syncing.
async fn refresh_counts(store: &Store, status: &watch::Sender<SyncStatus>) {
    let unsent = store.unsent_count().await.unwrap_or(0);
    let problems = store.note_problems().await.map_or(0, |list| list.len());
    let last_synced_ms = store.last_synced_ms().await.unwrap_or(None);
    status.send_if_modified(|status| {
        let changed = status.unsent != unsent
            || status.problems != problems
            || status.last_synced_ms != last_synced_ms;
        status.unsent = unsent;
        status.problems = problems;
        status.last_synced_ms = last_synced_ms;
        changed
    });
}
