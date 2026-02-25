//! Desktop session persistence using the OS keyring.

use dirt_core::auth::{AuthError, AuthResult, AuthSession, SessionPersistence};
use keyring::Entry;

const KEYRING_SERVICE_NAME: &str = "dirt";
const KEYRING_SESSION_USERNAME: &str = "supabase_session";

/// Desktop session store backed by the OS keyring (`keyring` crate).
#[derive(Debug, Clone)]
pub struct KeyringSessionStore {
    service_name: String,
    username: String,
}

impl Default for KeyringSessionStore {
    fn default() -> Self {
        Self {
            service_name: KEYRING_SERVICE_NAME.to_string(),
            username: KEYRING_SESSION_USERNAME.to_string(),
        }
    }
}

impl KeyringSessionStore {
    fn entry(&self) -> AuthResult<Entry> {
        Entry::new(&self.service_name, &self.username)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))
    }
}

impl SessionPersistence for KeyringSessionStore {
    fn load(&self) -> AuthResult<Option<AuthSession>> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }

    fn save(&self, session: &AuthSession) -> AuthResult<()> {
        let serialized = serde_json::to_string(session)?;
        self.entry()?
            .set_password(&serialized)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))
    }

    fn clear(&self) -> AuthResult<()> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }
}
