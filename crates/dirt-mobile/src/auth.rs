//! Supabase authentication service with secure session storage for mobile.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use crate::bootstrap_config::MobileBootstrapConfig;
use crate::secret_store;

use dirt_core::auth::{
    resolve_optional_supabase_config, AuthResult, SessionPersistence, SupabaseAuthClient,
};
#[allow(unused_imports)]
pub use dirt_core::auth::{AuthConfigStatus, AuthError, AuthSession, AuthUser, SignUpOutcome};

#[derive(Debug, Clone, Copy, Default)]
struct SessionStore;

impl SessionPersistence for SessionStore {
    fn load_session(&self) -> AuthResult<Option<AuthSession>> {
        match secret_store::read_secret(secret_store::SECRET_SUPABASE_SESSION) {
            Ok(Some(value)) => Ok(Some(serde_json::from_str(&value)?)),
            Ok(None) => Ok(None),
            Err(error) => Err(AuthError::SecureStorage(error)),
        }
    }

    fn save_session(&self, session: &AuthSession) -> AuthResult<()> {
        let serialized = serde_json::to_string(session)?;
        secret_store::write_secret(secret_store::SECRET_SUPABASE_SESSION, &serialized)
            .map_err(AuthError::SecureStorage)
    }

    fn clear_session(&self) -> AuthResult<()> {
        secret_store::delete_secret(secret_store::SECRET_SUPABASE_SESSION)
            .map_err(AuthError::SecureStorage)
    }
}

#[derive(Clone)]
pub struct SupabaseAuthService {
    inner: SupabaseAuthClient<SessionStore>,
}

impl SupabaseAuthService {
    pub fn new_from_bootstrap(config: &MobileBootstrapConfig) -> AuthResult<Option<Self>> {
        let Some((url, anon_key)) = resolve_optional_supabase_config(
            config.supabase_url.clone(),
            config.supabase_anon_key.clone(),
        )?
        else {
            return Ok(None);
        };

        Ok(Some(Self::new(url, anon_key)?))
    }

    pub fn new_from_env() -> AuthResult<Option<Self>> {
        let Some((url, anon_key)) = resolve_optional_supabase_config(
            std::env::var("SUPABASE_URL").ok(),
            std::env::var("SUPABASE_ANON_KEY").ok(),
        )?
        else {
            return Ok(None);
        };

        Ok(Some(Self::new(url, anon_key)?))
    }

    pub fn new(url: impl AsRef<str>, anon_key: impl Into<String>) -> AuthResult<Self> {
        Ok(Self {
            inner: SupabaseAuthClient::new(url, anon_key, SessionStore)?,
        })
    }

    pub async fn restore_session(&self) -> AuthResult<Option<AuthSession>> {
        self.inner.restore_session().await
    }

    pub async fn sign_up(&self, email: &str, password: &str) -> AuthResult<SignUpOutcome> {
        self.inner.sign_up(email, password).await
    }

    pub async fn sign_in(&self, email: &str, password: &str) -> AuthResult<AuthSession> {
        self.inner.sign_in(email, password).await
    }

    pub async fn refresh_session(&self, refresh_token: &str) -> AuthResult<AuthSession> {
        self.inner.refresh_session(refresh_token).await
    }

    pub async fn sign_out(&self, access_token: &str) -> AuthResult<()> {
        self.inner.sign_out(access_token).await
    }

    pub async fn verify_configuration(&self) -> AuthResult<AuthConfigStatus> {
        self.inner.verify_configuration().await
    }
}

#[cfg(test)]
mod tests {
    use dirt_core::auth::normalize_auth_url;

    use super::*;

    #[test]
    fn normalize_auth_url_appends_auth_path() {
        let normalized = normalize_auth_url("https://demo.supabase.co").unwrap();
        assert_eq!(normalized, "https://demo.supabase.co/auth/v1");
    }

    #[test]
    fn normalize_auth_url_keeps_existing_auth_path() {
        let normalized = normalize_auth_url("https://demo.supabase.co/auth/v1").unwrap();
        assert_eq!(normalized, "https://demo.supabase.co/auth/v1");
    }

    #[test]
    fn new_from_bootstrap_returns_none_when_values_missing() {
        let config = MobileBootstrapConfig::default();
        assert!(SupabaseAuthService::new_from_bootstrap(&config)
            .unwrap()
            .is_none());
    }
}
