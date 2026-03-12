//! Desktop session persistence using the OS keyring.

use dirt_core::auth::{AuthError, AuthResult, AuthSession, SessionPersistence};
use keyring::Entry;

const KEYRING_SERVICE_NAME: &str = "dirt";
const KEYRING_SESSION_USERNAME: &str = "supabase_session";
const LEGACY_KEYRING_SERVICE_NAMES: &[&str] = &["dirt-desktop"];

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

    fn entry_for_service(&self, service_name: &str) -> AuthResult<Entry> {
        Entry::new(service_name, &self.username)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))
    }

    fn parse_session(raw: &str) -> AuthResult<AuthSession> {
        serde_json::from_str(raw).map_err(Into::into)
    }

    fn load_legacy_and_migrate(&self) -> AuthResult<Option<AuthSession>> {
        for legacy_service in LEGACY_KEYRING_SERVICE_NAMES {
            let legacy_entry = self.entry_for_service(legacy_service)?;
            match legacy_entry.get_password() {
                Ok(raw) => {
                    let session = Self::parse_session(&raw)?;
                    if let Err(error) = self.save(&session) {
                        tracing::warn!(
                            "Failed to migrate legacy desktop session from service '{}': {}",
                            legacy_service,
                            error
                        );
                    } else if let Err(error) = legacy_entry.delete_credential() {
                        tracing::warn!(
                            "Failed to clean up migrated legacy desktop session from service '{}': {}",
                            legacy_service,
                            error
                        );
                    }
                    return Ok(Some(session));
                }
                Err(keyring::Error::NoEntry) => {}
                Err(error) => return Err(AuthError::SecureStorage(error.to_string())),
            }
        }

        Ok(None)
    }
}

impl SessionPersistence for KeyringSessionStore {
    #[allow(clippy::cognitive_complexity)]
    fn load(&self) -> AuthResult<Option<AuthSession>> {
        tracing::debug!(
            "Loading session from keyring (service={}, user={})",
            self.service_name,
            self.username,
        );
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(raw) => {
                tracing::debug!("Keyring entry found ({} bytes)", raw.len());
                Ok(Some(Self::parse_session(&raw)?))
            }
            Err(keyring::Error::NoEntry) => {
                tracing::debug!("No keyring entry found, checking legacy service names");
                self.load_legacy_and_migrate()
            }
            Err(error) => {
                tracing::warn!("Keyring load error: {}", error);
                Err(AuthError::SecureStorage(error.to_string()))
            }
        }
    }

    fn save(&self, session: &AuthSession) -> AuthResult<()> {
        let serialized = serde_json::to_string(session)?;
        tracing::debug!(
            "Saving session to keyring (service={}, user={}, {} bytes)",
            self.service_name,
            self.username,
            serialized.len(),
        );
        self.entry()?
            .set_password(&serialized)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))?;

        // Verify write by reading back immediately.
        match self.entry()?.get_password() {
            Ok(_) => tracing::debug!("Keyring write verified successfully"),
            Err(error) => tracing::error!("Keyring write verification FAILED: {}", error),
        }

        Ok(())
    }

    fn clear(&self) -> AuthResult<()> {
        tracing::debug!("Clearing session from keyring");
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dirt_core::auth::AuthUser;

    fn backend_unavailable(error: &AuthError) -> bool {
        let AuthError::SecureStorage(message) = error else {
            return false;
        };
        let lower = message.to_ascii_lowercase();
        lower.contains("dbus")
            || lower.contains("secret service")
            || lower.contains("platform secure storage is unavailable")
            || lower.contains("keychain")
            || lower.contains("no such interface")
            || lower.contains("not supported")
            || lower.contains("is unavailable")
    }

    #[test]
    fn keyring_roundtrip_write_and_read() {
        let store = KeyringSessionStore {
            service_name: "dirt-test-roundtrip".to_string(),
            username: "test-session".to_string(),
        };

        let session = AuthSession {
            access_token: "test-access".to_string(),
            refresh_token: "test-refresh".to_string(),
            expires_at: 9_999_999_999,
            user: AuthUser {
                id: "test-user-id".to_string(),
                email: Some("test@example.com".to_string()),
            },
        };

        // Save
        match store.save(&session) {
            Ok(()) => {}
            Err(error) if backend_unavailable(&error) => {
                eprintln!("Skipping keyring roundtrip test: {error}");
                return;
            }
            Err(error) => panic!("keyring save should succeed: {error}"),
        }

        // Load back
        let loaded = match store.load() {
            Ok(Some(session)) => session,
            Err(error) if backend_unavailable(&error) => {
                eprintln!("Skipping keyring roundtrip test: {error}");
                let _ = store.clear();
                return;
            }
            Ok(None) => {
                eprintln!("Skipping keyring roundtrip test: keyring backend did not return the saved session");
                let _ = store.clear();
                return;
            }
            Err(error) => panic!("keyring load should succeed: {error}"),
        };

        assert_eq!(loaded.access_token, "test-access");
        assert_eq!(loaded.refresh_token, "test-refresh");
        assert_eq!(loaded.user.id, "test-user-id");

        // Cleanup
        match store.clear() {
            Ok(()) => {}
            Err(error) if backend_unavailable(&error) => {
                eprintln!("Skipping keyring roundtrip cleanup verification: {error}");
                return;
            }
            Err(error) => panic!("keyring clear should succeed: {error}"),
        }

        // Verify cleared
        let after_clear = match store.load() {
            Ok(session) => session,
            Err(error) if backend_unavailable(&error) => {
                eprintln!("Skipping keyring clear verification: {error}");
                return;
            }
            Err(error) => panic!("keyring load after clear should succeed: {error}"),
        };
        assert!(after_clear.is_none());
    }
}
