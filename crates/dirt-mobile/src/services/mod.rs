//! Mobile shell services.
//!
//! The sign-in flow and the token stores it persists to. Neither depends
//! on Dioxus, so both are tested on the host.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

pub mod auth_flow;
pub mod auth_store;

// `DefaultTokenStore` is reached via its module path in `app_shell.rs`
// (`crate::services::auth_store::DefaultTokenStore`) so a top-level
// re-export here would be unused. Left commented as a marker that the
// alias lives one level deeper if a future caller needs a shorter path.
// pub use auth_store::DefaultTokenStore;
