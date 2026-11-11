//! Verifies an OpenID Connect ID token against the issuer's published keys.
//!
//! This is the whole of what the Dirt server needs from the identity
//! provider: proof that this user signed in, for this client, in answer to
//! this sign-in attempt. Nothing here is specific to Clerk.

use std::fmt;

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

/// Seconds of clock difference tolerated between us and the issuer.
const LEEWAY_SECONDS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdClaims {
    pub iss: String,
    /// The user's stable ID at the issuer. The server keys accounts on this.
    pub sub: String,
    pub exp: u64,
    #[serde(default)]
    pub nonce: Option<String>,
    /// Everything else the issuer put in the token.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug)]
pub enum VerifyError {
    /// Not a token at all, or its header cannot be read.
    Malformed(String),
    /// Signed with something other than RS256. Refused before looking at any
    /// key, so a token cannot choose a weaker check for itself.
    WrongAlgorithm(Algorithm),
    /// The token names no key, or one the issuer does not publish.
    UnknownKey(Option<String>),
    /// Bad signature, wrong issuer, wrong audience, expired, or a required
    /// claim is missing.
    Rejected(jsonwebtoken::errors::Error),
    /// Valid, but not issued for this sign-in attempt.
    WrongNonce,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(reason) => write!(f, "malformed token: {reason}"),
            Self::WrongAlgorithm(algorithm) => write!(f, "signed with {algorithm:?}, not RS256"),
            Self::UnknownKey(Some(id)) => write!(f, "the issuer publishes no key named {id}"),
            Self::UnknownKey(None) => write!(f, "the token names no key"),
            Self::Rejected(error) => write!(f, "rejected: {error}"),
            Self::WrongNonce => write!(f, "the nonce does not match this sign-in attempt"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Accepts `token` only if the issuer's key signed it with RS256, it names
/// `issuer` and `client_id`, it has not expired, and it carries `nonce`.
pub fn verify_id_token(
    token: &str,
    keys: &JwkSet,
    issuer: &str,
    client_id: &str,
    nonce: &str,
) -> Result<IdClaims, VerifyError> {
    let header = decode_header(token).map_err(|error| VerifyError::Malformed(error.to_string()))?;
    if header.alg != Algorithm::RS256 {
        return Err(VerifyError::WrongAlgorithm(header.alg));
    }
    let key_id = header.kid.ok_or(VerifyError::UnknownKey(None))?;
    let key = keys
        .find(&key_id)
        .ok_or_else(|| VerifyError::UnknownKey(Some(key_id.clone())))?;
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
    Ok(claims)
}
