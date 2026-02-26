//! CLI Supabase auth/session helpers with secure keychain persistence.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(not(test))]
use keyring::Entry;

use crate::config_profiles::CliProfile;

use dirt_core::auth::{
    resolve_optional_supabase_config, AuthResult, SessionPersistence, SupabaseAuthClient,
};
pub use dirt_core::auth::{AuthError, AuthSession};

#[cfg(not(test))]
const KEYRING_SERVICE_NAME: &str = "dirt-cli";

#[derive(Clone)]
struct SessionStore {
    username: String,
}

impl SessionStore {
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

impl SessionPersistence for SessionStore {
    #[cfg(not(test))]
    fn load_session(&self) -> AuthResult<Option<AuthSession>> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }

    #[cfg(test)]
    fn load_session(&self) -> AuthResult<Option<AuthSession>> {
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
    fn save_session(&self, session: &AuthSession) -> AuthResult<()> {
        let raw = serde_json::to_string(session)?;
        self.entry()?
            .set_password(&raw)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))?;
        Ok(())
    }

    #[cfg(test)]
    fn save_session(&self, session: &AuthSession) -> AuthResult<()> {
        let raw = serde_json::to_string(session)?;
        let store = Self::test_store();
        let mut guard = store
            .lock()
            .map_err(|error| AuthError::SecureStorage(error.to_string()))?;
        guard.insert(self.username.clone(), raw);
        Ok(())
    }

    #[cfg(not(test))]
    fn clear_session(&self) -> AuthResult<()> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }

    #[cfg(test)]
    fn clear_session(&self) -> AuthResult<()> {
        let store = Self::test_store();
        let mut guard = store
            .lock()
            .map_err(|error| AuthError::SecureStorage(error.to_string()))?;
        guard.remove(&self.username);
        Ok(())
    }
}

#[derive(Clone)]
pub struct SupabaseAuthService {
    inner: SupabaseAuthClient<SessionStore>,
}

impl SupabaseAuthService {
    pub fn new_for_profile(profile_name: &str, profile: &CliProfile) -> AuthResult<Option<Self>> {
        let Some((url, anon_key)) =
            resolve_optional_supabase_config(profile.supabase_url(), profile.supabase_anon_key())?
        else {
            return Ok(None);
        };

        Ok(Some(Self::new(profile_name, &url, &anon_key)?))
    }

    pub fn new(
        profile_name: &str,
        url: impl AsRef<str>,
        anon_key: impl AsRef<str>,
    ) -> AuthResult<Self> {
        Ok(Self {
            inner: SupabaseAuthClient::new(
                url,
                anon_key.as_ref().to_string(),
                SessionStore::new(profile_name),
            )?,
        })
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
    SessionStore::new(profile_name).load_session()
}

pub fn clear_stored_session(profile_name: &str) -> AuthResult<()> {
    SessionStore::new(profile_name).clear_session()
}

#[cfg(test)]
mod tests {
    use dirt_core::auth::normalize_auth_url;
    use dirt_core::auth::AuthUser;

    use super::*;

    #[test]
    fn normalize_auth_url_appends_auth_suffix() {
        let normalized = normalize_auth_url("https://demo.supabase.co").unwrap();
        assert_eq!(normalized, "https://demo.supabase.co/auth/v1");
    }

    #[test]
    fn normalize_auth_url_keeps_auth_suffix() {
        let normalized = normalize_auth_url("https://demo.supabase.co/auth/v1").unwrap();
        assert_eq!(normalized, "https://demo.supabase.co/auth/v1");
    }

    #[test]
    fn session_debug_redacts_tokens() {
        let session = AuthSession {
            access_token: "secret-access-token".to_string(),
            refresh_token: "secret-refresh-token".to_string(),
            expires_at: 1_700_000_000,
            user: AuthUser {
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
