//! Supabase authentication service for mobile.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

pub use dirt_core::auth::{
    AuthConfigStatus, AuthError, AuthResult, AuthSession, AuthUser, SignUpOutcome,
};
use dirt_core::auth::{SessionPersistence, SupabaseAuthService as CoreSupabaseAuthService};

use crate::bootstrap_config::MobileBootstrapConfig;
use crate::secret_store;

#[derive(Debug, Clone, Copy, Default)]
struct SessionStore;

impl SessionPersistence for SessionStore {
    fn load(&self) -> AuthResult<Option<AuthSession>> {
        match secret_store::read_secret(secret_store::SECRET_SUPABASE_SESSION) {
            Ok(Some(raw)) => Ok(Some(serde_json::from_str(&raw)?)),
            Ok(None) => Ok(None),
            Err(error) => Err(AuthError::SecureStorage(error)),
        }
    }

    fn save(&self, session: &AuthSession) -> AuthResult<()> {
        let serialized = serde_json::to_string(session)?;
        secret_store::write_secret(secret_store::SECRET_SUPABASE_SESSION, &serialized)
            .map_err(AuthError::SecureStorage)
    }

    fn clear(&self) -> AuthResult<()> {
        secret_store::delete_secret(secret_store::SECRET_SUPABASE_SESSION)
            .map_err(AuthError::SecureStorage)
    }
}

/// Supabase auth API client for mobile settings flows.
pub struct SupabaseAuthService {
    inner: CoreSupabaseAuthService<SessionStore>,
}

impl SupabaseAuthService {
    /// Create a service from build-time bootstrap config values.
    pub fn new_from_bootstrap(config: &MobileBootstrapConfig) -> AuthResult<Option<Self>> {
        Self::new_from_sources(
            config.supabase_url.clone(),
            config.supabase_anon_key.clone(),
        )
    }

    fn new_from_sources(url: Option<String>, anon_key: Option<String>) -> AuthResult<Option<Self>> {
        match (url, anon_key) {
            (None, None) => Ok(None),
            (Some(url), Some(anon_key)) => Ok(Some(Self::new(url, anon_key)?)),
            _ => Err(AuthError::NotConfigured),
        }
    }

    /// Create a service with explicit Supabase project URL and anon key.
    pub fn new(url: impl AsRef<str>, anon_key: impl Into<String>) -> AuthResult<Self> {
        let inner = CoreSupabaseAuthService::with_session_store(url, anon_key, SessionStore)?;
        Ok(Self { inner })
    }

    /// Restore session from secure storage. If expired, refresh automatically.
    pub async fn restore_session(&self) -> AuthResult<Option<AuthSession>> {
        self.inner.restore_session().await
    }

    /// Sign up a user by email/password.
    pub async fn sign_up(&self, email: &str, password: &str) -> AuthResult<SignUpOutcome> {
        self.inner.sign_up(email, password).await
    }

    /// Sign in an existing user by email/password.
    pub async fn sign_in(&self, email: &str, password: &str) -> AuthResult<AuthSession> {
        self.inner.sign_in(email, password).await
    }

    /// Refresh an access token using the refresh token.
    pub async fn refresh_session(&self, refresh_token: &str) -> AuthResult<AuthSession> {
        self.inner.refresh_session(refresh_token).await
    }

    /// Sign out and clear local session storage.
    pub async fn sign_out(&self, access_token: &str) -> AuthResult<()> {
        self.inner.sign_out(access_token).await
    }

    /// Verify Supabase auth configuration and return a summary for UI diagnostics.
    pub async fn verify_configuration(&self) -> AuthResult<AuthConfigStatus> {
        self.inner.verify_configuration().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap_config::MobileBootstrapConfig;

    #[test]
    fn new_from_bootstrap_returns_none_when_values_missing() {
        let config = MobileBootstrapConfig::default();
        assert!(SupabaseAuthService::new_from_bootstrap(&config)
            .unwrap()
            .is_none());
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
