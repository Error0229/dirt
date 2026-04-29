//! Mobile shell services.
//!
//! Each module here is a long-lived background actor that the UI does
//! not own directly: the UI holds a handle (or reads from a channel)
//! and the actor lives on the tokio side.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

pub mod sync_worker;

#[cfg_attr(not(target_os = "android"), allow(unused_imports))]
pub use sync_worker::{spawn_sync_worker, SyncEvent, SyncWorkerHandle};
