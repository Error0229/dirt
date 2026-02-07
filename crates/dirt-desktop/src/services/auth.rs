//! Supabase authentication service with secure session storage.

use std::time::{SystemTime, UNIX_EPOCH};

use keyring::Entry;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const KEYRING_SERVICE_NAME: &str = "dirt";
const KEYRING_SESSION_USERNAME: &str = "supabase_session";
const EXPIRY_SKEW_SECONDS: i64 = 60;

/// Authenticated user metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthUser {
    /// Provider user id.
    pub id: String,
    /// Optional email from provider profile.
    pub email: Option<String>,
}

/// Persisted session used for API authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(unix_timestamp_now())
    }

    #[must_use]
    const fn is_expired_at(&self, now_secs: i64) -> bool {
        self.expires_at <= now_secs + EXPIRY_SKEW_SECONDS
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

/// Errors from authentication and secure storage flows.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Supabase auth is not configured. Set SUPABASE_URL and SUPABASE_ANON_KEY.")]
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

#[derive(Debug, Clone)]
struct SessionStore {
    service_name: String,
    username: String,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self {
            service_name: KEYRING_SERVICE_NAME.to_string(),
            username: KEYRING_SESSION_USERNAME.to_string(),
        }
    }
}

impl SessionStore {
    fn entry(&self) -> AuthResult<Entry> {
        Entry::new(&self.service_name, &self.username)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))
    }

    fn load_session(&self) -> AuthResult<Option<AuthSession>> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }

    fn save_session(&self, session: &AuthSession) -> AuthResult<()> {
        let serialized = serde_json::to_string(session)?;
        self.entry()?
            .set_password(&serialized)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))
    }

    fn clear_session(&self) -> AuthResult<()> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }
}

/// Supabase auth API client.
#[derive(Clone)]
pub struct SupabaseAuthService {
    auth_url: String,
    anon_key: String,
    client: Client,
    session_store: SessionStore,
}

impl SupabaseAuthService {
    /// Create a service from `SUPABASE_URL` and `SUPABASE_ANON_KEY`.
    pub fn new_from_env() -> AuthResult<Option<Self>> {
        let url = std::env::var("SUPABASE_URL").ok();
        let anon_key = std::env::var("SUPABASE_ANON_KEY").ok();

        match (url, anon_key) {
            (None, None) => Ok(None),
            (Some(url), Some(anon_key)) => {
                let service = Self::new(url, anon_key)?;
                Ok(Some(service))
            }
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
            session_store: SessionStore::default(),
        })
    }

    /// Restore session from secure storage. If expired, refresh automatically.
    pub async fn restore_session(&self) -> AuthResult<Option<AuthSession>> {
        let Some(stored_session) = self.session_store.load_session()? else {
            return Ok(None);
        };

        if !stored_session.is_expired() {
            return Ok(Some(stored_session));
        }

        match self.refresh_session(&stored_session.refresh_token).await {
            Ok(refreshed) => {
                self.session_store.save_session(&refreshed)?;
                Ok(Some(refreshed))
            }
            Err(error) => {
                tracing::warn!("Failed to refresh persisted session: {}", error);
                self.session_store.clear_session()?;
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
                self.session_store.save_session(&session)?;
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
        self.session_store.save_session(&session)?;
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
        self.session_store.save_session(&session)?;
        Ok(session)
    }

    /// Sign out and clear local session storage.
    pub async fn sign_out(&self, access_token: &str) -> AuthResult<()> {
        let request = self
            .client
            .post(format!("{}/logout", self.auth_url))
            .header("apikey", &self.anon_key)
            .bearer_auth(access_token);
        let response = request.send().await?;
        if !(response.status().is_success() || response.status() == StatusCode::UNAUTHORIZED) {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AuthError::Api(parse_api_error(status, &body)));
        }

        self.session_store.clear_session()
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

impl SupabaseAuthService {
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

#[cfg(test)]
mod tests {
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

    #[test]
    fn session_expiry_uses_safety_skew() {
        let session = AuthSession {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            expires_at: 1_000,
            user: AuthUser {
                id: "user".to_string(),
                email: None,
            },
        };

        assert!(session.is_expired_at(940));
        assert!(!session.is_expired_at(900));
    }
}
