//! Android secure secret storage helpers for mobile runtime config.

use std::sync::{Arc, OnceLock};

use keyring_core::{CredentialStore, Entry, Error as KeyringError};

const SECRET_SERVICE_NAME: &str = "dirt-mobile";
/// Secret key used for the Turso auth token in secure storage.
pub const SECRET_TURSO_AUTH_TOKEN: &str = "turso_auth_token";
/// Secret key used for the serialized Supabase session payload.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub const SECRET_SUPABASE_SESSION: &str = "supabase_session";

type SecretResult<T> = Result<T, String>;

static STORE_INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// Persist a secret by key into secure storage.
///
/// The provided value is trimmed and must be non-empty.
pub fn write_secret(name: &str, value: &str) -> SecretResult<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err("secret value must not be empty".to_string());
    }

    let entry = entry(name)?;
    entry.set_password(value).map_err(map_keyring_error)
}

/// Read a secret by key from secure storage.
///
/// Returns `Ok(None)` when the key is not present.
pub fn read_secret(name: &str) -> SecretResult<Option<String>> {
    let entry = entry(name)?;
    match entry.get_password() {
        Ok(value) => {
            let normalized = value.trim();
            if normalized.is_empty() {
                Ok(None)
            } else {
                Ok(Some(normalized.to_string()))
            }
        }
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(map_keyring_error(error)),
    }
}

/// Check whether a secret exists for the provided key.
#[cfg_attr(not(test), allow(dead_code))]
pub fn has_secret(name: &str) -> SecretResult<bool> {
    Ok(read_secret(name)?.is_some())
}

/// Delete a secret by key from secure storage.
///
/// Missing entries are treated as a successful no-op.
pub fn delete_secret(name: &str) -> SecretResult<()> {
    let entry = entry(name)?;
    match entry.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(map_keyring_error(error)),
    }
}

fn entry(name: &str) -> SecretResult<Entry> {
    ensure_store()?;
    Entry::new(SECRET_SERVICE_NAME, name).map_err(map_keyring_error)
}

fn ensure_store() -> SecretResult<()> {
    STORE_INIT.get_or_init(initialize_store).clone()
}

#[cfg(target_os = "android")]
fn initialize_store() -> SecretResult<()> {
    let store: Arc<CredentialStore> = android_native_keyring_store::Store::new()
        .map_err(|error| format!("failed to initialize Android secure store: {error}"))?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn initialize_store() -> SecretResult<()> {
    let store: Arc<CredentialStore> = keyring_core::mock::Store::new()
        .map_err(|error| format!("failed to initialize mock secure store: {error}"))?;
    keyring_core::set_default_store(store);
    Ok(())
}

fn map_keyring_error(error: KeyringError) -> String {
    match error {
        KeyringError::NoDefaultStore => "secure store is not initialized".to_string(),
        KeyringError::NoEntry => "secret does not exist".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_roundtrip() {
        let key = "test-secret-roundtrip";
        delete_secret(key).unwrap();

        write_secret(key, " token ").unwrap();
        assert_eq!(read_secret(key).unwrap().as_deref(), Some("token"));
        assert!(has_secret(key).unwrap());

        delete_secret(key).unwrap();
        assert_eq!(read_secret(key).unwrap(), None);
    }

    #[test]
    fn empty_secret_is_rejected() {
        let key = "test-secret-empty";
        let error = write_secret(key, "   ").unwrap_err();
        assert!(error.contains("must not be empty"));
    }
}
