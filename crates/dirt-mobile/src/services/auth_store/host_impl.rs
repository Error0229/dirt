//! Host-side `TokenStore` used by `cargo test` on Windows / Linux.
//!
//! Never compiled into the Android APK — the `cfg(not(target_os =
//! "android"))` gate in `super` makes sure of that — so this code has no
//! security model beyond filesystem permissions. The Android shell only
//! cross-compiles via `cargo check --target x86_64-linux-android`, which
//! picks the `EncryptedPrefsTokenStore` path instead.
//!
//! Stored layout is a single JSON file per `(service, account)` slot at
//! `<data_dir>/dirt-host-mock/<service>__<account>.json`. The path
//! contains the slot identifiers literally so a developer can poke at
//! the stored value during tests / manual debugging.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(test)]
use dirt_core::auth::TokenStoreError;
use dirt_core::auth::{StoredToken, TokenStore, TokenStoreResult};

use super::{backend, parse_token_blob, serialize_token_blob};

/// File-backed `TokenStore`. The unencrypted JSON layout matches what
/// the desktop keyring stores in its slot (also unencrypted-from-our-POV
/// since the OS keyring owns the encryption), so a test can dump one and
/// inspect it with `jq`.
pub struct FileTokenStore {
    path: PathBuf,
}

impl std::fmt::Debug for FileTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileTokenStore")
            .field("path", &self.path)
            .finish()
    }
}

impl FileTokenStore {
    /// Open the slot for `(service, account)` under the user data dir.
    /// The directory is created lazily on first `save`; this constructor
    /// does not touch the filesystem so a test path that doesn't yet
    /// exist is fine.
    //
    // The `Result` return type is here for signature compatibility with
    // the Android `EncryptedPrefsTokenStore::open`, whose master-key
    // materialization is genuinely fallible (JNI / KeyStore can refuse).
    // Keeping both `open` signatures identical lets `app_shell` and the
    // settings flow handle construction errors uniformly across the
    // cfg-split — that's the whole reason the alias `DefaultTokenStore`
    // exists. So `unnecessary_wraps` is a false positive here.
    #[allow(clippy::unnecessary_wraps)]
    pub fn open(service: &str, account: &str) -> TokenStoreResult<Self> {
        let base = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("dirt-host-mock");
        Ok(Self {
            path: base.join(slot_filename(service, account)),
        })
    }

    /// Test-only constructor letting the caller pin the storage path
    /// (typically a `tempfile::TempDir` join). Production code paths go
    /// through [`open`].
    #[cfg(test)]
    #[must_use]
    pub const fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Inspect the resolved on-disk path. Useful in tests that need to
    /// poke the file directly (e.g. simulating a corrupted blob).
    #[cfg(test)]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl TokenStore for FileTokenStore {
    fn load(&self) -> TokenStoreResult<Option<StoredToken>> {
        match fs::read_to_string(&self.path) {
            Ok(json) => Ok(Some(parse_token_blob(&json)?)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(backend(format!("read {}: {err}", self.path.display()))),
        }
    }

    fn save(&self, token: &StoredToken) -> TokenStoreResult<()> {
        let json = serialize_token_blob(token)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| backend(format!("create {}: {err}", parent.display())))?;
        }
        fs::write(&self.path, json)
            .map_err(|err| backend(format!("write {}: {err}", self.path.display())))
    }

    fn clear(&self) -> TokenStoreResult<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(backend(format!("remove {}: {err}", self.path.display()))),
        }
    }
}

fn slot_filename(service: &str, account: &str) -> String {
    // Path-segment safe encoding: replace anything weird with `_` so a
    // colon in a service name doesn't blow up on Windows (which rejects
    // `:` in file paths). Keep the encoding obvious so a human reading
    // the dir can map back to the slot.
    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    format!("{}__{}.json", safe(service), safe(account))
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

    fn temp_store() -> (tempfile_lite::TempDir, FileTokenStore) {
        let dir = tempfile_lite::TempDir::new("dirt-host-store-test").unwrap();
        let store = FileTokenStore::with_path(dir.path().join("slot.json"));
        (dir, store)
    }

    #[test]
    fn load_returns_none_when_file_missing() {
        let (_dir, store) = temp_store();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let (_dir, store) = temp_store();
        store.save(&sample_token()).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded, sample_token());
    }

    #[test]
    fn save_overwrites_previous_blob() {
        let (_dir, store) = temp_store();
        store.save(&sample_token()).unwrap();
        // `StoredToken` implements `Drop` (via `ZeroizeOnDrop`) so the
        // `..sample_token()` struct-update sugar is rejected: it would
        // partially-move the source. Build the rotated token explicitly.
        let updated = StoredToken {
            session_token: "rotated".into(),
            session_id: "sid".into(),
            user_id: "uid".into(),
            email: "user@example.com".into(),
            expires_at_ms: 123,
        };
        store.save(&updated).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.session_token, "rotated");
    }

    #[test]
    fn clear_removes_existing_file() {
        let (_dir, store) = temp_store();
        store.save(&sample_token()).unwrap();
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn clear_is_idempotent_when_already_empty() {
        let (_dir, store) = temp_store();
        store.clear().unwrap(); // Pre-empty.
        store.save(&sample_token()).unwrap();
        store.clear().unwrap();
        store.clear().unwrap(); // Second clear must not fail.
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn load_classifies_malformed_blob_as_serialize_error() {
        let (_dir, store) = temp_store();
        // Force a corrupted blob into the slot to simulate an out-of-band
        // write (or a future schema mismatch). The caller treats this as
        // "force a fresh sign-in", same as the desktop keyring contract.
        std::fs::write(store.path(), "not json at all").unwrap();
        let err = store.load().unwrap_err();
        assert!(matches!(err, TokenStoreError::Serialize(_)));
    }

    #[test]
    fn slot_filename_escapes_unsafe_chars() {
        // Colons are illegal in Windows file paths; the encoder must
        // map them to `_` so the test running on Windows doesn't blow
        // up before the assertion.
        assert_eq!(
            slot_filename("dev.dirt.session", "default"),
            "dev.dirt.session__default.json"
        );
        assert_eq!(slot_filename("a:b/c", "x"), "a_b_c__x.json");
    }

    /// Minimal stand-in for the `tempfile` crate so the mobile crate
    /// doesn't pick up a workspace dep just for one test module. Wraps
    /// `std::env::temp_dir()` with a randomized subdirectory; cleans up
    /// on drop.
    mod tempfile_lite {
        use std::fs;
        use std::path::{Path, PathBuf};

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new(prefix: &str) -> std::io::Result<Self> {
                // 96 bits of entropy from the process clock keeps the
                // suffix short and collision-free for the test lifetime.
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
                fs::create_dir_all(&path)?;
                Ok(Self(path))
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }
}
