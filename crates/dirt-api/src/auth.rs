use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::http::HeaderMap;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub session_id: Option<String>,
}

#[derive(Clone)]
pub struct SupabaseJwtVerifier {
    client: reqwest::Client,
    config: Arc<AppConfig>,
    cache: Arc<RwLock<JwksCache>>,
}

impl SupabaseJwtVerifier {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            cache: Arc::new(RwLock::new(JwksCache::default())),
        }
    }

    pub async fn verify_access_token(&self, token: &str) -> Result<AuthenticatedUser, AppError> {
        let header = decode_header(token).map_err(|error| {
            AppError::unauthorized(format!("Token header decode failed: {}", sanitize(&error)))
        })?;
        let kid = header
            .kid
            .ok_or_else(|| AppError::unauthorized("Token header missing `kid`"))?;

        let key = self.find_key(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_aud = false;
        validation.set_issuer(&[self.config.supabase_jwt_issuer.as_str()]);

        let decoded = decode::<SupabaseClaims>(token, &key, &validation).map_err(|error| {
            AppError::unauthorized(format!("Token validation failed: {}", sanitize(&error)))
        })?;

        if !audience_matches(
            decoded.claims.aud.as_ref(),
            &self.config.supabase_jwt_audience,
        ) {
            return Err(AppError::unauthorized("Token audience is not allowed"));
        }
        if decoded.claims.sub.trim().is_empty() {
            return Err(AppError::unauthorized("Token subject is missing"));
        }
        if decoded.claims.role.as_deref() != Some("authenticated") {
            return Err(AppError::unauthorized("Token role is not allowed"));
        }
        validate_temporal_claims(&decoded.claims, self.config.auth_clock_skew)?;

        Ok(AuthenticatedUser {
            user_id: decoded.claims.sub,
            session_id: decoded.claims.session_id.or(decoded.claims.jti),
        })
    }

    async fn find_key(&self, kid: &str) -> Result<DecodingKey, AppError> {
        {
            let cache = self.cache.read().await;
            if !cache.is_stale(self.config.jwks_cache_ttl) {
                if let Some(key) = cache.keys.get(kid) {
                    return Ok(key.clone());
                }
            }
        }

        let mut cache = self.cache.write().await;
        if !cache.is_stale(self.config.jwks_cache_ttl) {
            if let Some(key) = cache.keys.get(kid) {
                return Ok(key.clone());
            }
        }

        let keys = fetch_jwks(&self.client, &self.config.supabase_jwks_url).await?;
        cache.keys = keys;
        cache.fetched_at = Some(Instant::now());

        cache
            .keys
            .get(kid)
            .cloned()
            .ok_or_else(|| AppError::unauthorized("Signing key not found in Supabase JWKS"))
    }
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AppError> {
    let header = headers
        .get("authorization")
        .ok_or_else(|| AppError::unauthorized("Missing Authorization header"))?
        .to_str()
        .map_err(|_| AppError::unauthorized("Authorization header is not valid UTF-8"))?;

    let (scheme, token) = header
        .split_once(' ')
        .ok_or_else(|| AppError::unauthorized("Authorization header must be `Bearer <token>`"))?;

    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AppError::unauthorized(
            "Authorization scheme must be `Bearer`",
        ));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::unauthorized("Bearer token is empty"));
    }

    Ok(token)
}

#[derive(Default)]
struct JwksCache {
    keys: HashMap<String, DecodingKey>,
    fetched_at: Option<Instant>,
}

impl JwksCache {
    fn is_stale(&self, ttl: std::time::Duration) -> bool {
        self.fetched_at.map_or(true, |at| at.elapsed() > ttl)
    }
}

#[derive(Debug, Deserialize)]
struct JwksDocument {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize)]
struct Jwk {
    kid: Option<String>,
    kty: Option<String>,
    use_: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SupabaseClaims {
    sub: String,
    aud: Option<Value>,
    role: Option<String>,
    exp: Option<i64>,
    iat: Option<i64>,
    nbf: Option<i64>,
    jti: Option<String>,
    session_id: Option<String>,
}

fn validate_temporal_claims(
    claims: &SupabaseClaims,
    clock_skew: std::time::Duration,
) -> Result<(), AppError> {
    let now = chrono::Utc::now().timestamp();
    let skew = i64::try_from(clock_skew.as_secs()).unwrap_or(0);

    let exp = claims
        .exp
        .ok_or_else(|| AppError::unauthorized("Token missing `exp` claim"))?;
    if exp <= now.saturating_sub(skew) {
        return Err(AppError::unauthorized("Token is expired"));
    }

    let iat = claims
        .iat
        .ok_or_else(|| AppError::unauthorized("Token missing `iat` claim"))?;
    if iat > now.saturating_add(skew) {
        return Err(AppError::unauthorized("Token `iat` is in the future"));
    }

    if let Some(nbf) = claims.nbf {
        if nbf > now.saturating_add(skew) {
            return Err(AppError::unauthorized("Token is not yet valid"));
        }
    }

    Ok(())
}

async fn fetch_jwks(
    client: &reqwest::Client,
    jwks_url: &str,
) -> Result<HashMap<String, DecodingKey>, AppError> {
    let response = client
        .get(jwks_url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| {
            AppError::external(format!("JWKS request failed: {}", sanitize(&error)))
        })?;

    if !response.status().is_success() {
        return Err(AppError::external(format!(
            "JWKS request failed with HTTP {}",
            response.status().as_u16()
        )));
    }

    let payload = response.json::<JwksDocument>().await.map_err(|error| {
        AppError::external(format!("JWKS JSON parse failed: {}", sanitize(&error)))
    })?;

    let mut out = HashMap::new();
    for key in payload.keys {
        let Some(kid) = key.kid else {
            continue;
        };
        if key.kty.as_deref() != Some("RSA") {
            continue;
        }
        if key.use_.as_deref().is_some_and(|usage| usage != "sig") {
            continue;
        }
        let Some(n) = key.n else {
            continue;
        };
        let Some(e) = key.e else {
            continue;
        };
        let decoding = DecodingKey::from_rsa_components(&n, &e).map_err(|error| {
            AppError::external(format!("Invalid JWKS RSA key: {}", sanitize(&error)))
        })?;
        out.insert(kid, decoding);
    }

    if out.is_empty() {
        return Err(AppError::external(
            "JWKS did not include any usable RSA signing keys",
        ));
    }

    Ok(out)
}

fn audience_matches(aud: Option<&Value>, expected: &str) -> bool {
    let Some(aud) = aud else {
        return false;
    };

    match aud {
        Value::String(value) => value == expected,
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .any(|value| value == expected),
        _ => false,
    }
}

fn sanitize(error: &impl std::fmt::Display) -> String {
    error.to_string().replace('\n', " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn bearer_token_extractor_accepts_standard_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer abc.def.ghi"),
        );

        assert_eq!(extract_bearer_token(&headers).unwrap(), "abc.def.ghi");
    }

    #[test]
    fn bearer_token_extractor_rejects_wrong_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Basic abc"));
        assert!(extract_bearer_token(&headers).is_err());
    }

    #[test]
    fn audience_matches_string_or_array() {
        assert!(audience_matches(
            Some(&Value::String("authenticated".to_string())),
            "authenticated"
        ));
        assert!(audience_matches(
            Some(&Value::Array(vec![
                Value::String("anon".to_string()),
                Value::String("authenticated".to_string())
            ])),
            "authenticated"
        ));
        assert!(!audience_matches(
            Some(&Value::String("anon".to_string())),
            "authenticated"
        ));
    }

    #[test]
    fn temporal_claims_require_exp_and_iat() {
        let claims = SupabaseClaims {
            sub: "user".to_string(),
            aud: Some(Value::String("authenticated".to_string())),
            role: Some("authenticated".to_string()),
            exp: None,
            iat: None,
            nbf: None,
            jti: None,
            session_id: None,
        };
        let err =
            validate_temporal_claims(&claims, std::time::Duration::from_secs(60)).unwrap_err();
        assert!(err.to_string().contains("missing `exp`"));
    }

    #[test]
    fn temporal_claims_reject_future_iat() {
        let now = chrono::Utc::now().timestamp();
        let claims = SupabaseClaims {
            sub: "user".to_string(),
            aud: Some(Value::String("authenticated".to_string())),
            role: Some("authenticated".to_string()),
            exp: Some(now + 300),
            iat: Some(now + 120),
            nbf: None,
            jti: None,
            session_id: None,
        };
        let err =
            validate_temporal_claims(&claims, std::time::Duration::from_secs(30)).unwrap_err();
        assert!(err.to_string().contains("future"));
    }
}
