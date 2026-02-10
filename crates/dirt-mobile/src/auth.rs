//! Supabase authentication service for mobile.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bootstrap_config::MobileBootstrapConfig;
use crate::secret_store;

const EXPIRY_SKEW_SECONDS: i64 = 60;

/// Authenticated user metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthUser {
    /// Provider user id.
    pub id: String,
    /// Optional user email.
    pub email: Option<String>,
}

/// Persisted auth session used for API authorization.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    /// Short-lived access token.
    pub access_token: String,
    /// Refresh token for obtaining new access tokens.
    pub refresh_token: String,
    /// Unix timestamp (seconds) when access token expires.
    pub expires_at: i64,
    /// Authenticated user profile.
    pub user: AuthUser,
}

impl AuthSession {
    /// Return whether the access token should be treated as expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(unix_timestamp_now())
    }

    #[must_use]
    const fn is_expired_at(&self, now_secs: i64) -> bool {
        self.expires_at <= now_secs + EXPIRY_SKEW_SECONDS
    }
}

impl fmt::Debug for AuthSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthSession")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("user", &self.user)
            .finish()
    }
}

/// Sign-up result from provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignUpOutcome {
    /// Account created and session started immediately.
    SignedIn(AuthSession),
    /// Account created but provider requires email confirmation.
    ConfirmationRequired,
}

/// Auth configuration status returned from Supabase settings endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct AuthConfigStatus {
    /// Whether email auth provider is enabled.
    pub email_enabled: bool,
    /// Whether sign-ups are allowed.
    pub signup_enabled: bool,
    /// Whether sign-ups are auto-confirmed without email delivery.
    pub mailer_autoconfirm: bool,
    /// Whether custom SMTP credentials are configured.
    pub smtp_configured: bool,
    /// Current Supabase email send rate limit (requests/hour).
    pub rate_limit_email_sent: Option<i64>,
}

/// Errors from authentication and secure storage flows.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error(
        "Supabase auth is not configured. Provide SUPABASE_URL and SUPABASE_ANON_KEY at build time."
    )]
    NotConfigured,
    #[error("Invalid auth configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Failed to parse JSON payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Auth API error: {0}")]
    Api(String),
    #[error("Secure storage error: {0}")]
    SecureStorage(String),
}

type AuthResult<T> = Result<T, AuthError>;

#[derive(Debug, Clone, Copy, Default)]
struct SessionStore;

impl SessionStore {
    fn load_session() -> AuthResult<Option<AuthSession>> {
        match secret_store::read_secret(secret_store::SECRET_SUPABASE_SESSION) {
            Ok(Some(raw)) => Ok(Some(serde_json::from_str(&raw)?)),
            Ok(None) => Ok(None),
            Err(error) => Err(AuthError::SecureStorage(error)),
        }
    }

    fn save_session(session: &AuthSession) -> AuthResult<()> {
        let serialized = serde_json::to_string(session)?;
        secret_store::write_secret(secret_store::SECRET_SUPABASE_SESSION, &serialized)
            .map_err(AuthError::SecureStorage)
    }

    fn clear_session() -> AuthResult<()> {
        secret_store::delete_secret(secret_store::SECRET_SUPABASE_SESSION)
            .map_err(AuthError::SecureStorage)
    }
}

/// Supabase auth API client for mobile settings flows.
#[derive(Clone)]
pub struct SupabaseAuthService {
    auth_url: String,
    anon_key: String,
    client: Client,
}

impl SupabaseAuthService {
    /// Create a service from build-time bootstrap config values.
    pub fn new_from_bootstrap(config: &MobileBootstrapConfig) -> AuthResult<Option<Self>> {
        Self::new_from_sources(
            config.supabase_url.clone(),
            config.supabase_anon_key.clone(),
        )
    }

    /// Create a service from `SUPABASE_URL` and `SUPABASE_ANON_KEY`.
    pub fn new_from_env() -> AuthResult<Option<Self>> {
        Self::new_from_sources(
            std::env::var("SUPABASE_URL").ok(),
            std::env::var("SUPABASE_ANON_KEY").ok(),
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
        let auth_url = normalize_auth_url(url.as_ref())?;
        let anon_key = anon_key.into().trim().to_string();
        if anon_key.is_empty() {
            return Err(AuthError::InvalidConfiguration(
                "SUPABASE_ANON_KEY must not be empty",
            ));
        }

        let client = Client::builder().build()?;

        Ok(Self {
            auth_url,
            anon_key,
            client,
        })
    }

    /// Restore session from secure storage. If expired, refresh automatically.
    pub async fn restore_session(&self) -> AuthResult<Option<AuthSession>> {
        let Some(stored_session) = SessionStore::load_session()? else {
            return Ok(None);
        };

        if !stored_session.is_expired() {
            return Ok(Some(stored_session));
        }

        match self.refresh_session(&stored_session.refresh_token).await {
            Ok(refreshed) => {
                SessionStore::save_session(&refreshed)?;
                Ok(Some(refreshed))
            }
            Err(error) => {
                tracing::warn!("Failed to refresh persisted session: {}", error);
                SessionStore::clear_session()?;
                Ok(None)
            }
        }
    }

    /// Sign up a user by email/password.
    pub async fn sign_up(&self, email: &str, password: &str) -> AuthResult<SignUpOutcome> {
        validate_credentials(email, password)?;

        let payload = serde_json::json!({
            "email": email,
            "password": password,
        });
        let request = self.public_request(
            self.client
                .post(format!("{}/signup", self.auth_url))
                .json(&payload),
        );
        let response = self.send_auth_request(request).await?;
        match response.into_session()? {
            Some(session) => {
                SessionStore::save_session(&session)?;
                Ok(SignUpOutcome::SignedIn(session))
            }
            None => Ok(SignUpOutcome::ConfirmationRequired),
        }
    }

    /// Sign in an existing user by email/password.
    pub async fn sign_in(&self, email: &str, password: &str) -> AuthResult<AuthSession> {
        validate_credentials(email, password)?;

        let payload = serde_json::json!({
            "email": email,
            "password": password,
        });
        let request = self.public_request(
            self.client
                .post(format!("{}/token", self.auth_url))
                .query(&[("grant_type", "password")])
                .json(&payload),
        );
        let response = self.send_auth_request(request).await?;
        let session = response.into_session()?.ok_or_else(|| {
            AuthError::Api("Sign-in response did not include an active session".to_string())
        })?;
        SessionStore::save_session(&session)?;
        Ok(session)
    }

    /// Refresh an access token using the refresh token.
    pub async fn refresh_session(&self, refresh_token: &str) -> AuthResult<AuthSession> {
        if refresh_token.trim().is_empty() {
            return Err(AuthError::InvalidConfiguration(
                "Refresh token must not be empty",
            ));
        }

        let payload = serde_json::json!({
            "refresh_token": refresh_token,
        });
        let request = self.public_request(
            self.client
                .post(format!("{}/token", self.auth_url))
                .query(&[("grant_type", "refresh_token")])
                .json(&payload),
        );
        let response = self.send_auth_request(request).await?;
        let session = response.into_session()?.ok_or_else(|| {
            AuthError::Api("Refresh response did not include an active session".to_string())
        })?;
        SessionStore::save_session(&session)?;
        Ok(session)
    }

    /// Sign out and clear local session storage.
    pub async fn sign_out(&self, access_token: &str) -> AuthResult<()> {
        let server_logout = async {
            let request = self
                .client
                .post(format!("{}/logout", self.auth_url))
                .header("apikey", &self.anon_key)
                .bearer_auth(access_token);
            let response = request.send().await?;
            if response.status().is_success() || response.status() == StatusCode::UNAUTHORIZED {
                Ok(())
            } else {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                Err(AuthError::Api(parse_api_error(status, &body)))
            }
        }
        .await;

        SessionStore::clear_session()?;

        if let Err(error) = server_logout {
            tracing::warn!(
                "Server logout failed after clearing local session: {}",
                error
            );
        }

        Ok(())
    }

    /// Verify Supabase auth configuration and return a summary for UI diagnostics.
    pub async fn verify_configuration(&self) -> AuthResult<AuthConfigStatus> {
        let request = self.public_request(self.client.get(format!("{}/settings", self.auth_url)));
        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AuthError::Api(parse_api_error(status, &body)));
        }

        let payload = response.json::<SupabaseAuthSettings>().await?;
        Ok(AuthConfigStatus {
            email_enabled: payload.external.email,
            signup_enabled: !payload.disable_signup,
            mailer_autoconfirm: payload.mailer_autoconfirm,
            smtp_configured: payload.smtp_host.is_some(),
            rate_limit_email_sent: payload.rate_limit_email_sent,
        })
    }

    fn public_request(&self, request: RequestBuilder) -> RequestBuilder {
        request
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
    }

    async fn send_auth_request(&self, request: RequestBuilder) -> AuthResult<SupabaseAuthResponse> {
        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AuthError::Api(parse_api_error(status, &body)));
        }

        Ok(response.json::<SupabaseAuthResponse>().await?)
    }
}

#[derive(Debug, Deserialize)]
struct SupabaseAuthResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    expires_in: Option<i64>,
    user: Option<SupabaseUser>,
    session: Option<SupabaseAuthResponseSession>,
}

impl SupabaseAuthResponse {
    fn into_session(self) -> AuthResult<Option<AuthSession>> {
        let session = self.session;
        let access_token = self
            .access_token
            .or_else(|| session.as_ref().and_then(|s| s.access_token.clone()));
        let refresh_token = self
            .refresh_token
            .or_else(|| session.as_ref().and_then(|s| s.refresh_token.clone()));
        let expires_at = self
            .expires_at
            .or_else(|| session.as_ref().and_then(|s| s.expires_at))
            .or_else(|| {
                self.expires_in
                    .or_else(|| session.as_ref().and_then(|s| s.expires_in))
                    .map(|expires_in| unix_timestamp_now().saturating_add(expires_in))
            });
        let user = self
            .user
            .or_else(|| session.and_then(|s| s.user))
            .map(Into::into);

        match (access_token, refresh_token, expires_at, user) {
            (Some(access_token), Some(refresh_token), Some(expires_at), Some(user)) => {
                Ok(Some(AuthSession {
                    access_token,
                    refresh_token,
                    expires_at,
                    user,
                }))
            }
            (None, None, None, Some(_)) => Ok(None),
            _ => Err(AuthError::Api(
                "Auth response did not include enough session fields".to_string(),
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
struct SupabaseAuthResponseSession {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    expires_in: Option<i64>,
    user: Option<SupabaseUser>,
}

#[derive(Debug, Deserialize)]
struct SupabaseUser {
    id: String,
    email: Option<String>,
}

impl From<SupabaseUser> for AuthUser {
    fn from(value: SupabaseUser) -> Self {
        Self {
            id: value.id,
            email: value.email,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SupabaseErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
    message: Option<String>,
    msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SupabaseAuthSettings {
    external: SupabaseExternalSettings,
    disable_signup: bool,
    mailer_autoconfirm: bool,
    smtp_host: Option<String>,
    rate_limit_email_sent: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SupabaseExternalSettings {
    email: bool,
}

fn parse_api_error(status: StatusCode, body: &str) -> String {
    if let Ok(payload) = serde_json::from_str::<SupabaseErrorResponse>(body) {
        if let Some(message) = payload
            .message
            .or(payload.msg)
            .or(payload.error_description)
            .or(payload.error)
        {
            return format!("{} ({})", message.trim(), status.as_u16());
        }
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        format!("HTTP {}", status.as_u16())
    } else {
        format!("{} ({})", trimmed, status.as_u16())
    }
}

fn normalize_auth_url(url: &str) -> AuthResult<String> {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(AuthError::InvalidConfiguration(
            "SUPABASE_URL must not be empty",
        ));
    }
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err(AuthError::InvalidConfiguration(
            "SUPABASE_URL must include http:// or https://",
        ));
    }

    if trimmed.ends_with("/auth/v1") {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("{trimmed}/auth/v1"))
    }
}

fn validate_credentials(email: &str, password: &str) -> AuthResult<()> {
    if email.trim().is_empty() {
        return Err(AuthError::Api("Email is required".to_string()));
    }
    if password.trim().is_empty() {
        return Err(AuthError::Api("Password is required".to_string()));
    }
    Ok(())
}

fn unix_timestamp_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap_config::MobileBootstrapConfig;

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

    #[test]
    fn response_without_session_fields_means_confirmation_required() {
        let response = SupabaseAuthResponse {
            access_token: None,
            refresh_token: None,
            expires_at: None,
            expires_in: None,
            user: Some(SupabaseUser {
                id: "user-id".to_string(),
                email: Some("test@example.com".to_string()),
            }),
            session: None,
        };

        let session = response.into_session().unwrap();
        assert!(session.is_none());
    }
}
