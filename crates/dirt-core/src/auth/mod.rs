//! Magic-link auth client + persistent session storage.
//!
//! Phase 2.4 of the auth migration. This module ships two
//! independent-but-complementary pieces:
//!
//!   - [`AuthClient`] — typed wrapper around the four endpoints under
//!     `/v1/auth/*` exposed by `dirt-api::routes_auth`
//!     (`request`, `verify`, `refresh`, `logout`). No credentials are
//!     held on the client; anonymous endpoints take no auth, and bearer
//!     endpoints take the session token as a method parameter.
//!
//!   - [`TokenStore`] — pluggable storage for the `StoredToken`
//!     returned by `verify_magic_code`. Ships with
//!     [`MemoryTokenStore`] (tests and short-lived processes) and —
//!     on desktop targets — [`KeyringTokenStore`], a thin wrapper over
//!     the platform-native secret store (Keychain / Credential Manager
//!     / Secret Service via the `keyring` crate).
//!
//! Downstream binaries (`dirt-cli`, `dirt-desktop`, `dirt-mobile`)
//! compose these directly: stand up an [`AuthClient`] against
//! `DIRT_API_BASE_URL`, drive the `request → verify` flow, persist the
//! returned [`StoredToken`] via a [`TokenStore`], and feed the saved
//! `session_token` into [`ApiClient`](crate::sync::api_client::ApiClient)
//! for note sync.
//!
//! Android intentionally does not get `KeyringTokenStore` — the
//! `keyring` crate has no Android backend, and `dirt-mobile` will
//! provide its own Android KeyStore-backed implementation in P2.7.

mod client;
mod memory_store;
mod token_store;

// `KeyringTokenStore` lives behind the `keyring-store` Cargo feature
// (default on for dirt-core itself; opt-out for `dirt-api` so the
// Vercel server build doesn't drag in libdbus on Linux) AND a
// non-Android target gate (the `keyring` crate has no Android backend;
// `dirt-mobile` ships its own Android KeyStore implementation in P2.7).
#[cfg(all(feature = "keyring-store", not(target_os = "android")))]
mod keyring_store;

pub use client::{
    AuthClient, AuthError, AuthResult, RefreshResponse, RequestResponse, VerifyResponse,
};
pub use memory_store::MemoryTokenStore;
pub use token_store::{StoredToken, TokenStore, TokenStoreError, TokenStoreResult};

#[cfg(all(feature = "keyring-store", not(target_os = "android")))]
pub use keyring_store::KeyringTokenStore;
