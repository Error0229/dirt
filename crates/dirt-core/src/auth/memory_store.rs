//! In-memory token store.
//!
//! Intended for tests and short-lived processes (CLI smoke runs,
//! integration harnesses). The stored token disappears when the
//! `MemoryTokenStore` is dropped; nothing touches disk or the OS
//! keyring.
//!
//! For real applications, use
//! [`KeyringTokenStore`](super::KeyringTokenStore) so the session
//! survives across app restarts.

use std::sync::Mutex;

use super::{StoredToken, TokenStore, TokenStoreError, TokenStoreResult};

#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    inner: Mutex<Option<StoredToken>>,
}

impl MemoryTokenStore {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Seed the store with an initial token. Useful for tests that need
    /// to verify "what happens if `load()` returns `Some(token)` at app
    /// start" without going through a full verify round-trip first.
    #[must_use]
    pub const fn with_initial(token: StoredToken) -> Self {
        Self {
            inner: Mutex::new(Some(token)),
        }
    }
}

impl TokenStore for MemoryTokenStore {
    fn load(&self) -> TokenStoreResult<Option<StoredToken>> {
        let guard = self
            .inner
            .lock()
            .map_err(|err| TokenStoreError::Backend(err.to_string()))?;
        Ok(guard.clone())
    }

    fn save(&self, token: &StoredToken) -> TokenStoreResult<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|err| TokenStoreError::Backend(err.to_string()))?;
        *guard = Some(token.clone());
        Ok(())
    }

    fn clear(&self) -> TokenStoreResult<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|err| TokenStoreError::Backend(err.to_string()))?;
        *guard = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_token() -> StoredToken {
        StoredToken {
            session_token: "tok".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 123,
        }
    }

    #[test]
    fn fresh_store_loads_none() {
        let store = MemoryTokenStore::new();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = MemoryTokenStore::new();
        let token = sample_token();
        store.save(&token).unwrap();
        assert_eq!(store.load().unwrap(), Some(token));
    }

    #[test]
    fn save_overwrites_existing_token() {
        let store = MemoryTokenStore::new();
        store.save(&sample_token()).unwrap();

        let next = StoredToken {
            session_token: "tok2".into(),
            ..sample_token()
        };
        store.save(&next).unwrap();
        assert_eq!(store.load().unwrap().unwrap().session_token, "tok2");
    }

    #[test]
    fn clear_empties_the_store() {
        let store = MemoryTokenStore::with_initial(sample_token());
        assert!(store.load().unwrap().is_some());
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn clear_on_empty_store_is_noop() {
        let store = MemoryTokenStore::new();
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn with_initial_seeds_the_store() {
        let store = MemoryTokenStore::with_initial(sample_token());
        assert_eq!(store.load().unwrap(), Some(sample_token()));
    }
}
