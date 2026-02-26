//! CLI auth/session helpers backed by `dirt-core::auth`.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use dirt_core::auth::{
    AuthError, AuthResult, AuthSession, SessionPersistence,
    SupabaseAuthService as CoreSupabaseAuthService,
};

#[cfg(not(test))]
use keyring::Entry;

use crate::config_profiles::CliProfile;

#[cfg(not(test))]
const KEYRING_SERVICE_NAME: &str = "dirt-cli";

#[derive(Clone)]
struct ProfileSessionStore {
    username: String,
}

impl ProfileSessionStore {
    fn new(profile_name: &str) -> Self {
        Self {
            username: format!("supabase_session:{profile_name}"),
        }
    }

    #[cfg(test)]
    fn test_store() -> &'static Mutex<HashMap<String, String>> {
        static STORE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    #[cfg(not(test))]
    fn entry(&self) -> AuthResult<Entry> {
        Entry::new(KEYRING_SERVICE_NAME, &self.username)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))
    }
}

impl SessionPersistence for ProfileSessionStore {
    #[cfg(not(test))]
    fn load(&self) -> AuthResult<Option<AuthSession>> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }

    #[cfg(test)]
    fn load(&self) -> AuthResult<Option<AuthSession>> {
        let store = Self::test_store();
        let guard = store
            .lock()
            .map_err(|error| AuthError::SecureStorage(error.to_string()))?;
        if let Some(raw) = guard.get(&self.username) {
            Ok(Some(serde_json::from_str(raw)?))
        } else {
            Ok(None)
        }
    }

    #[cfg(not(test))]
    fn save(&self, session: &AuthSession) -> AuthResult<()> {
        let raw = serde_json::to_string(session)?;
        self.entry()?
            .set_password(&raw)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))?;
        Ok(())
    }

    #[cfg(test)]
    fn save(&self, session: &AuthSession) -> AuthResult<()> {
        let raw = serde_json::to_string(session)?;
        let store = Self::test_store();
        let mut guard = store
            .lock()
            .map_err(|error| AuthError::SecureStorage(error.to_string()))?;
        guard.insert(self.username.clone(), raw);
        Ok(())
    }

    #[cfg(not(test))]
    fn clear(&self) -> AuthResult<()> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }

    #[cfg(test)]
    fn clear(&self) -> AuthResult<()> {
        let store = Self::test_store();
        let mut guard = store
            .lock()
            .map_err(|error| AuthError::SecureStorage(error.to_string()))?;
        guard.remove(&self.username);
        Ok(())
    }
}

/// CLI auth service that reuses shared `dirt-core` auth logic.
pub struct SupabaseAuthService {
    inner: CoreSupabaseAuthService<ProfileSessionStore>,
}

impl SupabaseAuthService {
    /// Builds a profile-scoped auth service from profile config values.
    pub fn new_for_profile(profile_name: &str, profile: &CliProfile) -> AuthResult<Option<Self>> {
        let url = profile.supabase_url();
        let anon_key = profile.supabase_anon_key();

        match (url, anon_key) {
            (None, None) => Ok(None),
            (Some(url), Some(anon_key)) => {
                let inner = CoreSupabaseAuthService::with_session_store(
                    url,
                    anon_key,
                    ProfileSessionStore::new(profile_name),
                )?;
                Ok(Some(Self { inner }))
            }
            _ => Err(AuthError::NotConfigured),
        }
    }

    pub async fn sign_in(&self, email: &str, password: &str) -> AuthResult<AuthSession> {
        self.inner.sign_in(email, password).await
    }

    pub async fn restore_session(&self) -> AuthResult<Option<AuthSession>> {
        self.inner.restore_session().await
    }

    pub async fn refresh_session(&self, refresh_token: &str) -> AuthResult<AuthSession> {
        self.inner.refresh_session(refresh_token).await
    }

    pub async fn sign_out(&self, access_token: &str) -> AuthResult<()> {
        self.inner.sign_out(access_token).await
    }
}

pub fn load_stored_session(profile_name: &str) -> AuthResult<Option<AuthSession>> {
    ProfileSessionStore::new(profile_name).load()
}

pub fn clear_stored_session(profile_name: &str) -> AuthResult<()> {
    ProfileSessionStore::new(profile_name).clear()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_profiles::CliProfile;

    #[test]
    fn new_for_profile_returns_none_when_values_missing() {
        let profile = CliProfile::default();
        assert!(SupabaseAuthService::new_for_profile("default", &profile)
            .unwrap()
            .is_none());
    }

    #[test]
    fn session_debug_redacts_tokens() {
        let session = AuthSession {
            access_token: "secret-access-token".to_string(),
            refresh_token: "secret-refresh-token".to_string(),
            expires_at: 1_700_000_000,
            user: dirt_core::auth::AuthUser {
                id: "user".to_string(),
                email: None,
            },
        };
        let rendered = format!("{session:?}");
        assert!(!rendered.contains("secret-access-token"));
        assert!(!rendered.contains("secret-refresh-token"));
        assert!(rendered.contains("[REDACTED]"));
    }
}
