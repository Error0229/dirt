//! CLI Supabase auth/session helpers with secure keychain persistence.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use keyring::Entry;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config_profiles::{is_http_url, CliProfile};

const KEYRING_SERVICE_NAME: &str = "dirt-cli";
const EXPIRY_SKEW_SECONDS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user: AuthUser,
}

impl AuthSession {
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_at <= unix_timestamp_now() + EXPIRY_SKEW_SECONDS
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

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Auth is not configured for profile '{0}'")]
    NotConfigured(String),
    #[error("Invalid auth configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Auth API error: {0}")]
    Api(String),
    #[error("Secure storage error: {0}")]
    SecureStorage(String),
}

type AuthResult<T> = Result<T, AuthError>;

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

    fn entry(&self) -> AuthResult<Entry> {
        Entry::new(KEYRING_SERVICE_NAME, &self.username)
            .map_err(|error| AuthError::SecureStorage(error.to_string()))
    }

    fn load(&self) -> AuthResult<Option<AuthSession>> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AuthError::SecureStorage(error.to_string())),
        }
    }

    fn save(&self, session: &AuthSession) -> AuthResult<()> {
        let raw = serde_json::to_string(session)?;
        self.entry()?
            .set_password(&raw)
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

#[derive(Clone)]
pub struct SupabaseAuthService {
    auth_url: String,
    anon_key: String,
    client: Client,
    store: SessionStore,
}

impl SupabaseAuthService {
    pub fn new_for_profile(profile_name: &str, profile: &CliProfile) -> AuthResult<Option<Self>> {
        let url = profile.supabase_url();
        let anon_key = profile.supabase_anon_key();

        match (url, anon_key) {
            (None, None) => Ok(None),
            (Some(url), Some(anon_key)) => Self::new(profile_name, &url, &anon_key).map(Some),
            _ => Err(AuthError::NotConfigured(profile_name.to_string())),
        }
    }

    pub fn new(
        profile_name: &str,
        url: impl AsRef<str>,
        anon_key: impl AsRef<str>,
    ) -> AuthResult<Self> {
        let auth_url = normalize_auth_url(url.as_ref())?;
        let anon_key = anon_key.as_ref().trim().to_string();
        if anon_key.is_empty() {
            return Err(AuthError::InvalidConfiguration(
                "Supabase anon key must not be empty",
            ));
        }

        Ok(Self {
            auth_url,
            anon_key,
            client: Client::builder().build()?,
            store: SessionStore::new(profile_name),
        })
    }

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
        self.store.save(&session)?;
        Ok(session)
    }

    pub async fn restore_session(&self) -> AuthResult<Option<AuthSession>> {
        let Some(stored_session) = self.store.load()? else {
            return Ok(None);
        };

        if !stored_session.is_expired() {
            return Ok(Some(stored_session));
        }

        match self.refresh_session(&stored_session.refresh_token).await {
            Ok(session) => Ok(Some(session)),
            Err(error) => {
                tracing::warn!("Failed to refresh CLI auth session: {}", error);
                self.store.clear()?;
                Ok(None)
            }
        }
    }

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
        self.store.save(&session)?;
        Ok(session)
    }

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

        self.store.clear()
    }
}

pub fn load_stored_session(profile_name: &str) -> AuthResult<Option<AuthSession>> {
    SessionStore::new(profile_name).load()
}

pub fn clear_stored_session(profile_name: &str) -> AuthResult<()> {
    SessionStore::new(profile_name).clear()
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
        let nested_session = self.session;
        let access_token = self
            .access_token
            .or_else(|| nested_session.as_ref().and_then(|s| s.access_token.clone()));
        let refresh_token = self.refresh_token.or_else(|| {
            nested_session
                .as_ref()
                .and_then(|s| s.refresh_token.clone())
        });
        let expires_at = self
            .expires_at
            .or_else(|| nested_session.as_ref().and_then(|s| s.expires_at))
            .or_else(|| {
                self.expires_in
                    .or_else(|| nested_session.as_ref().and_then(|s| s.expires_in))
                    .map(|expires_in| unix_timestamp_now().saturating_add(expires_in))
            });
        let user = self
            .user
            .or_else(|| nested_session.and_then(|s| s.user))
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
            "Supabase URL must not be empty",
        ));
    }
    if !is_http_url(trimmed) {
        return Err(AuthError::InvalidConfiguration(
            "Supabase URL must include http:// or https://",
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
    fn response_with_only_user_returns_confirmation_required() {
        let response = SupabaseAuthResponse {
            access_token: None,
            refresh_token: None,
            expires_at: None,
            expires_in: None,
            user: Some(SupabaseUser {
                id: "user".to_string(),
                email: Some("user@example.com".to_string()),
            }),
            session: None,
        };
        assert!(response.into_session().unwrap().is_none());
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
