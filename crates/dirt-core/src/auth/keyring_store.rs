//! Platform-native secret-store-backed token store.
//!
//! Uses the [`keyring`] crate so the underlying storage is the
//! OS-native secret store:
//!
//!   - macOS  → Keychain (via `apple-native`)
//!   - Windows → Credential Manager (via `windows-native`)
//!   - Linux   → Secret Service / `DBus` (via `sync-secret-service`)
//!
//! Android is intentionally excluded — `dirt-mobile` reaches for Android
//! `KeyStore` via JNI and brings its own [`TokenStore`] implementation
//! in Phase 2.7. The `keyring` dep is target-gated in `Cargo.toml` so
//! the Android cross-compile pipeline does not need to know about
//! secret-service / libdbus.
//!
//! The stored value is JSON-serialized [`StoredToken`] — keeping the
//! keyring slot a single opaque blob means new `StoredToken` fields
//! land without a migration on the platform store.

use keyring::Entry;

use super::{StoredToken, TokenStore, TokenStoreError, TokenStoreResult};

pub struct KeyringTokenStore {
    service: String,
    account: String,
}

impl std::fmt::Debug for KeyringTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `account` doubles as the user-identity tag in some secret-store
        // UIs (Keychain Access etc.). It's not a secret — it's how the
        // user finds the entry to revoke it manually — but we render it
        // through `finish_non_exhaustive` so future private fields don't
        // leak into Debug by mistake.
        f.debug_struct("KeyringTokenStore")
            .field("service", &self.service)
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl KeyringTokenStore {
    /// Construct a store bound to a keyring `service` + `account`.
    ///
    /// Convention:
    ///   - `service`: reverse-DNS app identifier, e.g. `"dev.dirt.session"`.
    ///   - `account`: per-user discriminator, e.g. `"default"` for the
    ///     solo phase, or a user id once multi-user lands.
    ///
    /// Both strings show up in the platform secret-store UI so the user
    /// can find and revoke the entry manually.
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }

    fn entry(&self) -> TokenStoreResult<Entry> {
        Entry::new(&self.service, &self.account).map_err(|err| {
            TokenStoreError::Backend(format!(
                "failed to open keyring entry for service={} account={}: {err}",
                self.service, self.account
            ))
        })
    }
}

impl TokenStore for KeyringTokenStore {
    fn load(&self) -> TokenStoreResult<Option<StoredToken>> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(json) => {
                let token: StoredToken = serde_json::from_str(&json)
                    .map_err(|err| TokenStoreError::Serialize(err.to_string()))?;
                Ok(Some(token))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(TokenStoreError::Backend(err.to_string())),
        }
    }

    fn save(&self, token: &StoredToken) -> TokenStoreResult<()> {
        let entry = self.entry()?;
        let json = serde_json::to_string(token)
            .map_err(|err| TokenStoreError::Serialize(err.to_string()))?;
        entry
            .set_password(&json)
            .map_err(|err| TokenStoreError::Backend(err.to_string()))
    }

    fn clear(&self) -> TokenStoreResult<()> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(TokenStoreError::Backend(err.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construction must not touch the platform keyring — building the
    /// store is a synchronous, infallible operation in `new`. (`Entry`
    /// creation happens lazily inside each method so a failing backend
    /// only blows up at the actual `load` / `save` / `clear` call.)
    #[test]
    fn new_does_not_touch_the_backend() {
        let store = KeyringTokenStore::new("dev.dirt.session.test", "default");
        // Debug formatting is the only side effect we can rely on
        // without poking the OS keyring from inside a unit test.
        let rendered = format!("{store:?}");
        assert!(rendered.contains("dev.dirt.session.test"));
        assert!(rendered.contains("default"));
    }
}
