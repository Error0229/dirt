//! Platform services for the mobile app.

#[cfg(target_os = "android")]
pub mod activity;
#[cfg(target_os = "android")]
pub mod secure_storage;
