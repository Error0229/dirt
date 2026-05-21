//! Persistent session-token storage for the mobile shell.
//!
//! Two implementations live behind a `cfg(target_os = ...)` split:
//!
//!   - **Android** (`EncryptedPrefsTokenStore`) wraps `AndroidX`
//!     `EncryptedSharedPreferences` via JNI. The master key is generated
//!     inside the Android `KeyStore` (hardware-backed where available)
//!     under the `androidx_security_crypto` alias the framework owns;
//!     the bearer is encrypted with that key (AES-256/GCM for values,
//!     AES-256/SIV for the preference key) and lives in an app-private
//!     `SharedPreferences` file. The `service` argument becomes the
//!     preference file name; the `account` argument becomes the key
//!     inside it — matching the desktop keyring convention so the
//!     `(service, account) = ("dev.dirt.session", "default")` slot has
//!     one logical meaning across binaries.
//!
//!   - **Host** (`FileTokenStore`) writes the `StoredToken` JSON to a
//!     local file. It only ships when the crate is *built for the host
//!     target* — i.e. under `cargo test -p dirt-mobile`. The Android
//!     build target never includes it, so a packaged APK cannot
//!     accidentally fall back to unencrypted storage.
//!
//! Both implementations expose the same `open(service, account)`
//! constructor and implement [`dirt_core::auth::TokenStore`], so the
//! rest of the mobile shell only refers to [`DefaultTokenStore`].

#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use dirt_core::auth::{StoredToken, TokenStoreError, TokenStoreResult};

#[cfg(target_os = "android")]
mod android_impl;

#[cfg(not(target_os = "android"))]
mod host_impl;

#[cfg(target_os = "android")]
pub use android_impl::EncryptedPrefsTokenStore as DefaultTokenStore;

#[cfg(not(target_os = "android"))]
pub use host_impl::FileTokenStore as DefaultTokenStore;

/// Shared JSON codec for both impls. Pulled out so the unit tests can
/// exercise the serialize / parse edge without touching either backend
/// (matches the pattern in `dirt-core::auth::keyring_store`).
pub(super) fn parse_token_blob(json: &str) -> TokenStoreResult<StoredToken> {
    serde_json::from_str(json).map_err(|err| TokenStoreError::Serialize(err.to_string()))
}

pub(super) fn serialize_token_blob(token: &StoredToken) -> TokenStoreResult<String> {
    serde_json::to_string(token).map_err(|err| TokenStoreError::Serialize(err.to_string()))
}

/// Convenience constructor for the `Backend` error variant — both
/// platforms wrap a lot of IO / JNI errors and the call sites get
/// significantly shorter when they don't have to spell out the variant.
pub(super) const fn backend(msg: String) -> TokenStoreError {
    TokenStoreError::Backend(msg)
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    fn sample() -> StoredToken {
        StoredToken {
            session_token: "tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 123,
        }
    }

    /// Schema-drift canary: parse and serialize must round-trip. If a
    /// future `StoredToken` field rename breaks this, every consumer
    /// (Android prefs, host file, desktop keyring) silently fails to
    /// rehydrate at startup.
    #[test]
    fn serialize_then_parse_round_trips() {
        let blob = serialize_token_blob(&sample()).unwrap();
        let parsed = parse_token_blob(&blob).unwrap();
        assert_eq!(parsed, sample());
    }

    #[test]
    fn parse_rejects_malformed_json() {
        let err = parse_token_blob("not json").unwrap_err();
        assert!(matches!(err, TokenStoreError::Serialize(_)));
    }

    /// Older blob with missing fields: must classify as Serialize so the
    /// caller treats the slot as empty and forces a fresh sign-in, the
    /// same recovery the desktop keyring path uses.
    #[test]
    fn parse_rejects_wrong_shape() {
        let err = parse_token_blob("{}").unwrap_err();
        assert!(matches!(err, TokenStoreError::Serialize(_)));
    }
}
