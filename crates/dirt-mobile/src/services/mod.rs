//! Mobile shell services.
//!
//! Each module here is a long-lived background actor that the UI does
//! not own directly: the UI holds a handle (or reads from a channel)
//! and the actor lives on the tokio side.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

pub mod auth_flow;
pub mod auth_store;
pub mod sync_worker;

// `DefaultTokenStore` is reached via its module path in `app_shell.rs`
// (`crate::services::auth_store::DefaultTokenStore`) so a top-level
// re-export here would be unused. Left commented as a marker that the
// alias lives one level deeper if a future caller needs a shorter path.
// pub use auth_store::DefaultTokenStore;
#[cfg_attr(not(target_os = "android"), allow(unused_imports))]
pub use sync_worker::{spawn_sync_worker, SyncEvent, SyncWorkerHandle};
