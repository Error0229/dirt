//! Platform services for the mobile app.

#[cfg(target_os = "android")]
pub mod secure_storage;
#[cfg(target_os = "android")]
pub mod sign_in;
