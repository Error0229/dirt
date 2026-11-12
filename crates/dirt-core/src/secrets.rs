//! What a signed-in device must keep secret, and where it keeps it.
//!
//! A device holds three secrets for its account: the device token, the
//! device's private key and the account key. They are kept together as one
//! record in the platform's secure storage:
//!
//! - the operating system's keychain on desktop, shared by the desktop app
//!   and the CLI, which are one device;
//! - Android's encrypted preferences on Android, implemented in the mobile
//!   crate because it needs JNI;
//! - memory in tests.
//!
//! A [`SecretStore`] keeps one opaque string. [`DeviceSecrets`] is what that
//! string holds.

use std::collections::BTreeMap;
use std::sync::Mutex;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::crypto::{AccountKey, DeviceKeyPair, KEY_VERSION};
use crate::key_change::KeyRing;

/// Why secure storage could not be used.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The platform's storage refused or failed.
    #[error("the device's secure storage failed: {0}")]
    Backend(String),
    /// A record was found but is not one this version can read.
    #[error("the saved sign-in could not be read: {0}")]
    Unreadable(String),
}

/// One slot of secure storage, holding one string.
pub trait SecretStore: Send + Sync {
    /// The stored value, or `None` if the slot is empty.
    fn load(&self) -> Result<Option<String>, SecretError>;
    /// Replaces the stored value.
    fn save(&self, value: &str) -> Result<(), SecretError>;
    /// Empties the slot. Emptying an empty slot succeeds.
    fn clear(&self) -> Result<(), SecretError>;
}

/// The name of the slot for an account on `server_url`. Each server gets its
/// own slot, so a build pointed at another server never reads or replaces
/// the keys of an account held elsewhere.
#[must_use]
pub fn slot_name(server_url: &str) -> String {
    let trimmed = server_url.trim().trim_end_matches('/');
    let host = trimmed
        .split_once("://")
        .map_or(trimmed, |(_, rest)| rest)
        .to_ascii_lowercase();
    if host.is_empty() {
        "default".to_owned()
    } else {
        host
    }
}

/// Secure storage that lives in memory, for tests and for nothing else.
#[derive(Default)]
pub struct MemorySecrets(Mutex<Option<String>>);

impl MemorySecrets {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for MemorySecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemorySecrets").finish_non_exhaustive()
    }
}

impl SecretStore for MemorySecrets {
    fn load(&self) -> Result<Option<String>, SecretError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| SecretError::Backend("poisoned".to_owned()))?
            .clone())
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        *self
            .0
            .lock()
            .map_err(|_| SecretError::Backend("poisoned".to_owned()))? = Some(value.to_owned());
        Ok(())
    }

    fn clear(&self) -> Result<(), SecretError> {
        *self
            .0
            .lock()
            .map_err(|_| SecretError::Backend("poisoned".to_owned()))? = None;
        Ok(())
    }
}

/// The operating system's keychain: Keychain on macOS, Credential Manager on
/// Windows, the Secret Service on Linux.
#[cfg(all(feature = "keyring-store", not(target_os = "android")))]
#[derive(Debug)]
pub struct KeyringSecrets {
    account: String,
}

#[cfg(all(feature = "keyring-store", not(target_os = "android")))]
impl KeyringSecrets {
    /// The name the entry is filed under. It is shown in the system's own
    /// keychain tool, where the person can find and remove the entry.
    const SERVICE: &'static str = "dev.dirt.device";

    /// The slot for an account on `server_url`. Nothing is read until the
    /// first call.
    #[must_use]
    pub fn for_server(server_url: &str) -> Self {
        Self {
            account: slot_name(server_url),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(Self::SERVICE, &self.account)
            .map_err(|error| SecretError::Backend(error.to_string()))
    }
}

#[cfg(all(feature = "keyring-store", not(target_os = "android")))]
impl SecretStore for KeyringSecrets {
    fn load(&self) -> Result<Option<String>, SecretError> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretError::Backend(error.to_string())),
        }
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        self.entry()?
            .set_password(value)
            .map_err(|error| SecretError::Backend(error.to_string()))
    }

    fn clear(&self) -> Result<(), SecretError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretError::Backend(error.to_string())),
        }
    }
}

/// Everything a device keeps for its account.
#[derive(Clone)]
pub struct DeviceSecrets {
    pub account_id: Uuid,
    pub device_id: Uuid,
    /// The token that proves this device to the server.
    pub token: String,
    /// The device's key pair. The private half never leaves this record.
    pub device_key: DeviceKeyPair,
    /// The key that encrypts the account's notes. A device that is waiting
    /// to be approved does not have it yet.
    pub account_key: Option<AccountKey>,
    /// Which version of the account's key `account_key` is. The first key
    /// is version 1, and each change of the key is the next.
    pub key_version: i32,
    /// The account keys before `account_key` that this device still needs,
    /// by version: to read notes not yet sent again under the new key.
    pub older_keys: BTreeMap<i32, AccountKey>,
    /// Whether the server has said this device is approved.
    pub approved: bool,
}

impl std::fmt::Debug for DeviceSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceSecrets")
            .field("account_id", &self.account_id)
            .field("device_id", &self.device_id)
            .field("approved", &self.approved)
            .field("has_account_key", &self.account_key.is_some())
            .field("key_version", &self.key_version)
            .finish_non_exhaustive()
    }
}

/// The record as it is stored. Keys are base64.
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    account_id: Uuid,
    device_id: Uuid,
    token: String,
    device_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_key: Option<String>,
    /// Absent in a record written before the account's key could change:
    /// the key in it is the first.
    #[serde(default = "first_key_version")]
    key_version: i32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    older_keys: BTreeMap<i32, String>,
    approved: bool,
}

const RECORD_VERSION: u32 = 1;

const fn first_key_version() -> i32 {
    KEY_VERSION
}

impl DeviceSecrets {
    /// Whether this device can read and write the account's notes: it is
    /// approved and holds the account key.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.approved && self.account_key.is_some()
    }

    /// The account keys this device holds, or `None` while it is waiting.
    #[must_use]
    pub fn keys(&self) -> Option<KeyRing> {
        self.account_key
            .clone()
            .map(|current| KeyRing::from_parts(self.key_version, current, self.older_keys.clone()))
    }

    /// Replaces the account keys this device holds.
    pub fn set_keys(&mut self, keys: &KeyRing) {
        self.account_key = Some(keys.current().clone());
        self.key_version = keys.version();
        self.older_keys = keys.older().clone();
    }

    /// Gives the device `key` as the account's key of `version`, and no
    /// other: what a device holds when it is let in, and after a reset.
    pub fn set_only_key(&mut self, version: i32, key: AccountKey) {
        self.set_keys(&KeyRing::at(version, key));
    }

    /// Reads the record from `store`, or `None` if nobody is signed in.
    pub fn read(store: &dyn SecretStore) -> Result<Option<Self>, SecretError> {
        let Some(text) = store.load()?.map(Zeroizing::new) else {
            return Ok(None);
        };
        let record: Record = serde_json::from_str(&text)
            .map_err(|error| SecretError::Unreadable(error.to_string()))?;
        if record.version != RECORD_VERSION {
            return Err(SecretError::Unreadable(format!(
                "it was written as version {}, and this app reads version {RECORD_VERSION}",
                record.version
            )));
        }
        let decode = |text: &str| {
            STANDARD
                .decode(text)
                .map(Zeroizing::new)
                .map_err(|error| SecretError::Unreadable(error.to_string()))
        };
        let device_key = DeviceKeyPair::from_secret_bytes(&decode(&record.device_key)?)
            .map_err(|error| SecretError::Unreadable(error.to_string()))?;
        let account_key_of = |text: &str| {
            AccountKey::from_bytes(&decode(text)?)
                .map_err(|error| SecretError::Unreadable(error.to_string()))
        };
        let account_key = match &record.account_key {
            Some(text) => Some(account_key_of(text)?),
            None => None,
        };
        let mut older_keys = BTreeMap::new();
        for (version, text) in &record.older_keys {
            older_keys.insert(*version, account_key_of(text)?);
        }
        Ok(Some(Self {
            account_id: record.account_id,
            device_id: record.device_id,
            token: record.token.clone(),
            device_key,
            account_key,
            key_version: record.key_version,
            older_keys,
            approved: record.approved,
        }))
    }

    /// Writes the record to `store`, replacing what was there.
    pub fn write(&self, store: &dyn SecretStore) -> Result<(), SecretError> {
        let record = Record {
            version: RECORD_VERSION,
            account_id: self.account_id,
            device_id: self.device_id,
            token: self.token.clone(),
            device_key: STANDARD.encode(Zeroizing::new(self.device_key.secret_bytes())),
            account_key: self
                .account_key
                .as_ref()
                .map(|key| STANDARD.encode(key.as_bytes())),
            key_version: self.key_version,
            older_keys: self
                .older_keys
                .iter()
                .map(|(version, key)| (*version, STANDARD.encode(key.as_bytes())))
                .collect(),
            approved: self.approved,
        };
        let text = Zeroizing::new(
            serde_json::to_string(&record)
                .map_err(|error| SecretError::Unreadable(error.to_string()))?,
        );
        store.save(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(account_key: Option<AccountKey>) -> DeviceSecrets {
        DeviceSecrets {
            account_id: Uuid::now_v7(),
            device_id: Uuid::now_v7(),
            token: "a-device-token".to_owned(),
            device_key: DeviceKeyPair::generate(),
            approved: account_key.is_some(),
            account_key,
            key_version: KEY_VERSION,
            older_keys: BTreeMap::new(),
        }
    }

    #[test]
    fn a_record_survives_being_stored_and_read() {
        let store = MemorySecrets::new();
        assert!(DeviceSecrets::read(&store).unwrap().is_none());

        let written = sample(Some(AccountKey::generate()));
        written.write(&store).unwrap();
        let read = DeviceSecrets::read(&store).unwrap().unwrap();

        assert_eq!(read.account_id, written.account_id);
        assert_eq!(read.device_id, written.device_id);
        assert_eq!(read.token, written.token);
        assert_eq!(
            read.device_key.secret_bytes(),
            written.device_key.secret_bytes()
        );
        assert_eq!(
            read.account_key.as_ref().unwrap().as_bytes(),
            written.account_key.as_ref().unwrap().as_bytes()
        );
        assert!(read.is_ready());
    }

    #[test]
    fn the_keys_before_the_current_one_survive_being_stored_and_read() {
        let store = MemorySecrets::new();
        let mut keys = KeyRing::first(AccountKey::generate());
        keys.adopt(AccountKey::generate());
        keys.adopt(AccountKey::generate());
        let mut written = sample(Some(AccountKey::generate()));
        written.set_keys(&keys);
        written.write(&store).unwrap();

        let read = DeviceSecrets::read(&store).unwrap().unwrap();
        assert_eq!(read.key_version, 3);
        assert!(read.keys().unwrap().same_as(&keys));
        assert_eq!(read.older_keys.len(), 2);
    }

    #[test]
    fn a_record_written_before_keys_had_versions_holds_the_first_key() {
        let store = MemorySecrets::new();
        let written = sample(Some(AccountKey::generate()));
        written.write(&store).unwrap();
        // As an earlier version of the app wrote it: no version, no older
        // keys.
        let mut record: serde_json::Value =
            serde_json::from_str(&store.load().unwrap().unwrap()).unwrap();
        let fields = record.as_object_mut().unwrap();
        fields.remove("key_version");
        fields.remove("older_keys");
        store.save(&record.to_string()).unwrap();

        let read = DeviceSecrets::read(&store).unwrap().unwrap();
        assert_eq!(read.key_version, KEY_VERSION);
        assert!(read.older_keys.is_empty());
        assert_eq!(
            read.account_key.unwrap().as_bytes(),
            written.account_key.unwrap().as_bytes()
        );
    }

    #[test]
    fn a_device_without_the_account_key_is_not_ready() {
        let store = MemorySecrets::new();
        sample(None).write(&store).unwrap();
        let read = DeviceSecrets::read(&store).unwrap().unwrap();
        assert!(read.account_key.is_none());
        assert!(!read.is_ready());

        // Approved by the server but still without the key is not ready
        // either: there is nothing to decrypt with.
        let mut approved = sample(None);
        approved.approved = true;
        assert!(!approved.is_ready());
    }

    #[test]
    fn a_record_this_version_cannot_read_is_an_error_not_a_sign_out() {
        let store = MemorySecrets::new();
        for text in [
            "not json",
            "{}",
            r#"{"version":2,"account_id":"0192f0c8-0000-7000-8000-000000000001","device_id":"0192f0c8-0000-7000-8000-000000000002","token":"t","device_key":"AAAA","approved":true}"#,
        ] {
            store.save(text).unwrap();
            let error = DeviceSecrets::read(&store).unwrap_err();
            assert!(matches!(error, SecretError::Unreadable(_)), "{text}");
        }
    }

    #[test]
    fn nothing_secret_is_printed() {
        let secrets = sample(Some(AccountKey::generate()));
        let printed = format!("{secrets:?}");
        assert!(!printed.contains("a-device-token"));
        assert!(!printed.contains(&STANDARD.encode(secrets.device_key.secret_bytes())));
    }

    #[test]
    fn each_server_has_its_own_slot() {
        assert_eq!(
            slot_name("https://dirt.example.com/"),
            slot_name("https://DIRT.example.com")
        );
        assert_ne!(
            slot_name("https://dirt.example.com"),
            slot_name("http://127.0.0.1:8080")
        );
        assert_eq!(slot_name("  "), "default");
    }

    #[test]
    fn clearing_signs_the_device_out() {
        let store = MemorySecrets::new();
        sample(None).write(&store).unwrap();
        store.clear().unwrap();
        store.clear().unwrap();
        assert!(DeviceSecrets::read(&store).unwrap().is_none());
    }
}
