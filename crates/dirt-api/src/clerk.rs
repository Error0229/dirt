//! Checking a Clerk sign-in.
//!
//! The server needs one thing from the identity provider: proof that this
//! person signed in, for our client, in answer to this sign-in attempt. That
//! proof is an `OpenID` Connect ID token, checked here against the provider's
//! published keys. Nothing in the check is specific to Clerk. The function
//! and its test cases come from trial 3 of the redesign.

use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::Mutex;

/// Seconds of clock difference tolerated between us and the issuer.
const LEEWAY_SECONDS: u64 = 60;

/// How long fetched keys are used before they are fetched again.
const KEY_LIFETIME: Duration = Duration::from_secs(3600);

/// The shortest gap between two fetches, so that tokens naming unknown keys
/// cannot make the server hammer the issuer.
const MIN_REFETCH_GAP: Duration = Duration::from_secs(60);

/// Who signed in, as the issuer names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignIn {
    /// The person's stable ID at the issuer. Accounts are keyed on it.
    pub user_id: String,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdClaims {
    sub: String,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// Not a token at all, or its header cannot be read.
    #[error("malformed token: {0}")]
    Malformed(String),
    /// Signed with something other than RS256. Refused before looking at
    /// any key, so a token cannot choose a weaker check for itself.
    #[error("signed with {0:?}, not RS256")]
    WrongAlgorithm(Algorithm),
    /// The token names no key, or one the issuer does not publish.
    #[error("the issuer publishes no such signing key")]
    UnknownKey,
    /// Bad signature, wrong issuer, wrong audience, expired, or a required
    /// claim is missing.
    #[error("rejected: {0}")]
    Rejected(jsonwebtoken::errors::Error),
    /// Valid, but not issued for this sign-in attempt.
    #[error("the nonce does not match this sign-in attempt")]
    WrongNonce,
    /// The issuer's keys could not be fetched. Says nothing about the token.
    #[error("could not fetch the issuer's signing keys: {0}")]
    KeysUnavailable(String),
}

/// Accepts `token` only if one of `keys` signed it with RS256, it names
/// `issuer` and `client_id`, it has not expired, and it carries `nonce`.
pub fn verify_id_token(
    token: &str,
    keys: &JwkSet,
    issuer: &str,
    client_id: &str,
    nonce: &str,
) -> Result<SignIn, VerifyError> {
    let header = decode_header(token).map_err(|error| VerifyError::Malformed(error.to_string()))?;
    if header.alg != Algorithm::RS256 {
        return Err(VerifyError::WrongAlgorithm(header.alg));
    }
    let key = header
        .kid
        .as_deref()
        .and_then(|id| keys.find(id))
        .ok_or(VerifyError::UnknownKey)?;
    let key = DecodingKey::from_jwk(key).map_err(VerifyError::Rejected)?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.leeway = LEEWAY_SECONDS;

    let claims = decode::<IdClaims>(token, &key, &validation)
        .map_err(VerifyError::Rejected)?
        .claims;
    if claims.nonce.as_deref() != Some(nonce) {
        return Err(VerifyError::WrongNonce);
    }
    Ok(SignIn {
        user_id: claims.sub,
        email: claims.email,
    })
}

/// Where the issuer's signing keys come from.
pub enum Keys {
    /// A fixed set. Tests use this with a key made on the spot.
    Fixed(JwkSet),
    /// Fetched from the issuer and kept in memory.
    Remote(RemoteKeys),
}

pub struct RemoteKeys {
    issuer: String,
    http: reqwest::Client,
    cached: Mutex<Option<(Instant, JwkSet)>>,
}

#[derive(Deserialize)]
struct Discovery {
    jwks_uri: String,
}

impl RemoteKeys {
    async fn fetch(&self) -> Result<JwkSet, VerifyError> {
        let unavailable = |error: reqwest::Error| VerifyError::KeysUnavailable(error.to_string());
        let discovery: Discovery = self
            .http
            .get(format!("{}/.well-known/openid-configuration", self.issuer))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unavailable)?
            .json()
            .await
            .map_err(unavailable)?;
        self.http
            .get(discovery.jwks_uri)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unavailable)?
            .json()
            .await
            .map_err(unavailable)
    }

    /// The keys, fetched if there are none yet, if they are old, or if
    /// `need_fresh` is set and the last fetch was not just now.
    async fn keys(&self, need_fresh: bool) -> Result<JwkSet, VerifyError> {
        let mut cached = self.cached.lock().await;
        if let Some((fetched, keys)) = cached.as_ref() {
            let age = fetched.elapsed();
            let usable = if need_fresh {
                age < MIN_REFETCH_GAP
            } else {
                age < KEY_LIFETIME
            };
            if usable {
                return Ok(keys.clone());
            }
        }
        let keys = self.fetch().await?;
        *cached = Some((Instant::now(), keys.clone()));
        Ok(keys)
    }
}

impl Keys {
    /// Keys fetched from `issuer` on first use.
    pub fn remote(issuer: &str) -> Result<Self, reqwest::Error> {
        Ok(Self::Remote(RemoteKeys {
            issuer: issuer.trim_end_matches('/').to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            cached: Mutex::new(None),
        }))
    }

    /// Checks an ID token against these keys.
    ///
    /// The issuer may start signing with a new key at any time. A token that
    /// names a key we do not have therefore triggers one fresh fetch before
    /// it is refused.
    pub async fn verify(
        &self,
        token: &str,
        issuer: &str,
        client_id: &str,
        nonce: &str,
    ) -> Result<SignIn, VerifyError> {
        match self {
            Self::Fixed(keys) => verify_id_token(token, keys, issuer, client_id, nonce),
            Self::Remote(remote) => {
                let keys = remote.keys(false).await?;
                match verify_id_token(token, &keys, issuer, client_id, nonce) {
                    Err(VerifyError::UnknownKey) => {
                        let keys = remote.keys(true).await?;
                        verify_id_token(token, &keys, issuer, client_id, nonce)
                    }
                    other => other,
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "clerk_tests.rs"]
mod tests;
