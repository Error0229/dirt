//! The client side of sign-in: authorization code with PKCE, returning
//! through a loopback address. No client secret exists anywhere.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::jwk::JwkSet;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use url::Url;

pub type AnyError = Box<dyn std::error::Error + Send + Sync>;

/// The parts of `{issuer}/.well-known/openid-configuration` the flow uses.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
    pub jwks_uri: String,
}

pub async fn discover(http: &reqwest::Client, issuer: &str) -> Result<Discovery, AnyError> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    Ok(http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub async fn fetch_keys(http: &reqwest::Client, discovery: &Discovery) -> Result<JwkSet, AnyError> {
    Ok(http
        .get(&discovery.jwks_uri)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// 32 random bytes as URL-safe text: 43 characters, 256 bits.
#[must_use]
pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

/// The S256 challenge for a PKCE verifier.
#[must_use]
pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// One sign-in attempt. In the real client the verifier must be stored
/// before the browser opens, because the app may be killed while it is open.
#[derive(Debug, Clone)]
pub struct Attempt {
    pub verifier: String,
    pub state: String,
    pub nonce: String,
    pub redirect_uri: String,
}

impl Attempt {
    #[must_use]
    pub fn new(redirect_uri: String) -> Self {
        Self {
            verifier: random_token(),
            state: random_token(),
            nonce: random_token(),
            redirect_uri,
        }
    }

    pub fn authorization_url(
        &self,
        discovery: &Discovery,
        client_id: &str,
        scopes: &str,
    ) -> Result<Url, AnyError> {
        let mut url = Url::parse(&discovery.authorization_endpoint)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("scope", scopes)
            .append_pair("state", &self.state)
            .append_pair("nonce", &self.nonce)
            .append_pair("code_challenge", &challenge_for(&self.verifier))
            .append_pair("code_challenge_method", "S256");
        Ok(url)
    }
}

/// A listener on a port the system picked, and the redirect address for it.
pub async fn bind_loopback() -> Result<(TcpListener, String), AnyError> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    Ok((listener, format!("http://127.0.0.1:{port}/callback")))
}

/// Waits for the browser to come back, answers it, and returns the code.
/// Requests for anything but `/callback` (a favicon, say) are answered and
/// ignored. A callback with the wrong `state` is an error.
pub async fn wait_for_code(
    listener: TcpListener,
    expected_state: &str,
    timeout: Duration,
) -> Result<String, AnyError> {
    tokio::time::timeout(timeout, async {
        loop {
            let (stream, _) = listener.accept().await?;
            let mut stream = BufReader::new(stream);
            let mut request_line = String::new();
            stream.read_line(&mut request_line).await?;
            // "GET /callback?code=...&state=... HTTP/1.1"
            let target = request_line.split_whitespace().nth(1).unwrap_or("/");
            let url = Url::parse(&format!("http://127.0.0.1{target}"))?;

            let outcome = if url.path() == "/callback" {
                let find = |name: &str| {
                    url.query_pairs()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value.into_owned())
                };
                Some(match (find("code"), find("state"), find("error")) {
                    (_, _, Some(error)) => Err(format!(
                        "the provider returned {error}: {}",
                        find("error_description").unwrap_or_default()
                    )),
                    (Some(code), Some(state), None) if state == expected_state => Ok(code),
                    (Some(_), _, None) => Err("the state does not match this attempt".to_owned()),
                    (None, _, None) => Err("the callback carries no code".to_owned()),
                })
            } else {
                None
            };

            let (status, text) = match &outcome {
                Some(Ok(_)) => ("200 OK", "Signed in. You can close this tab."),
                Some(Err(_)) => ("400 Bad Request", "Sign-in failed. Return to the app."),
                None => ("404 Not Found", "Not found."),
            };
            let body =
                format!("<!doctype html><meta charset=utf-8><title>Dirt</title><p>{text}</p>");
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.get_mut().write_all(response.as_bytes()).await?;
            stream.get_mut().shutdown().await.ok();

            match outcome {
                Some(Ok(code)) => return Ok::<_, AnyError>(code),
                Some(Err(reason)) => return Err(reason.into()),
                None => {}
            }
        }
    })
    .await
    .map_err(|_| "timed out waiting for the browser to return")?
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Exchanges the code. The PKCE verifier is the only proof sent: this is a
/// public client and has no secret.
pub async fn exchange_code(
    http: &reqwest::Client,
    discovery: &Discovery,
    client_id: &str,
    attempt: &Attempt,
    code: &str,
) -> Result<TokenResponse, AnyError> {
    let response = http
        .post(&discovery.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", attempt.redirect_uri.as_str()),
            ("client_id", client_id),
            ("code_verifier", attempt.verifier.as_str()),
        ])
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(format!("token endpoint answered {status}: {text}").into());
    }
    Ok(serde_json::from_str(&text)?)
}

/// The claims of a token that has the three-part signed shape, without
/// checking its signature. For printing only.
#[must_use]
pub fn peek_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}
