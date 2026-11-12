//! Signing in: the browser side of proving who the person is.
//!
//! The identity provider (Clerk) proves who is signing in and does nothing
//! else. This module runs the OAuth authorization-code flow with PKCE as a
//! public client, which has no secret, and ends with an ID token. That token
//! goes to the Dirt server once, to register the device; see
//! [`session`](crate::session).
//!
//! A sign-in is an [`Attempt`]:
//!
//! 1. The server issues a single-use nonce and names the provider.
//! 2. The app opens the system browser on the address from
//!    [`IdentityClient::authorization_url`].
//! 3. The provider sends the browser back to the attempt's redirect address.
//!    On desktop that is a [`Loopback`] listener on `127.0.0.1`; on Android
//!    it is a custom URL scheme the app is registered for.
//! 4. [`IdentityClient::redeem`] checks what came back and exchanges it for
//!    the ID token.
//!
//! The provider also returns an access token and a refresh token. They are
//! never read: nothing here has a field to put them in.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dirt_proto::api::NonceResponse;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::crypto::random_token;

/// The scopes asked for. `openid` is what makes the provider return an ID
/// token; `email` puts the address in it, which the server keeps so that a
/// person can recognise their account.
const SCOPES: &str = "openid email";

/// The path the provider sends the browser back to on the loopback address.
const CALLBACK_PATH: &str = "/callback";

/// Why a sign-in did not complete.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The provider could not be reached, or answered with something that
    /// is not what the protocol says.
    #[error("could not reach the sign-in service: {0}")]
    Provider(String),
    /// What came back does not belong to this attempt, or the provider
    /// refused the sign-in.
    #[error("sign-in was not completed: {0}")]
    Rejected(String),
    /// The address the server named for sign-in is not one this app will
    /// send a person to.
    #[error("the sign-in address is not acceptable: {0}")]
    BadAddress(String),
    /// The local listener for the browser's return could not be used.
    #[error("could not listen for the browser to return: {0}")]
    Listener(#[from] std::io::Error),
}

/// One sign-in attempt. It can be saved and loaded: Android may stop the app
/// while the browser is in front, and the restarted app must be able to
/// finish what it began.
#[derive(Clone, Serialize, Deserialize)]
pub struct Attempt {
    /// The provider's issuer address.
    pub issuer: String,
    /// The OAuth client ID. It is a public client: there is no secret.
    pub client_id: String,
    /// Where the provider sends the browser afterwards.
    pub redirect_uri: String,
    /// The server's single-use nonce, which ends up inside the ID token.
    pub nonce: String,
    /// Returned by the provider unchanged; ties the redirect to this
    /// attempt.
    pub state: String,
    /// The PKCE secret. Only its hash is sent with the browser; the value
    /// itself is sent when the code is exchanged, so a stolen redirect is
    /// useless to anyone else.
    verifier: String,
}

impl std::fmt::Debug for Attempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attempt")
            .field("issuer", &self.issuer)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

impl Attempt {
    /// Starts an attempt with the nonce and provider the server gave.
    pub fn new(
        server: &NonceResponse,
        redirect_uri: impl Into<String>,
    ) -> Result<Self, IdentityError> {
        require_secure(&server.issuer)?;
        Ok(Self {
            issuer: server.issuer.trim_end_matches('/').to_owned(),
            client_id: server.client_id.clone(),
            redirect_uri: redirect_uri.into(),
            nonce: server.nonce.clone(),
            state: random_token(),
            verifier: random_token(),
        })
    }

    fn challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()))
    }
}

/// An address must be HTTPS, unless it is this machine or the Android
/// emulator's name for its host, which development uses.
fn require_secure(address: &str) -> Result<Url, IdentityError> {
    let url = Url::parse(address).map_err(|error| IdentityError::BadAddress(error.to_string()))?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "10.0.2.2"));
    match url.scheme() {
        "https" => Ok(url),
        "http" if local => Ok(url),
        _ => Err(IdentityError::BadAddress(format!(
            "{address} is not an https address"
        ))),
    }
}

/// The parts of the provider's discovery document that the flow uses.
#[derive(Debug, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    /// Whether the provider adds `iss` to its redirects (RFC 9207).
    #[serde(default)]
    authorization_response_iss_parameter_supported: bool,
}

/// The token endpoint's answer. Only the ID token is read.
#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    id_token: Option<String>,
}

/// Talks to the identity provider.
#[derive(Debug, Clone)]
pub struct IdentityClient {
    http: reqwest::Client,
}

impl IdentityClient {
    pub fn new() -> Result<Self, IdentityError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| IdentityError::Provider(error.to_string()))?;
        Ok(Self { http })
    }

    async fn discover(&self, issuer: &str) -> Result<Discovery, IdentityError> {
        let address = format!("{issuer}/.well-known/openid-configuration");
        let provider = |error: reqwest::Error| IdentityError::Provider(error.to_string());
        let discovery: Discovery = self
            .http
            .get(address)
            .send()
            .await
            .map_err(provider)?
            .error_for_status()
            .map_err(provider)?
            .json()
            .await
            .map_err(provider)?;
        // The document must describe the issuer that was asked for, and
        // must not send the person or the code somewhere insecure.
        if discovery.issuer.trim_end_matches('/') != issuer {
            return Err(IdentityError::Provider(format!(
                "{issuer} describes itself as {}",
                discovery.issuer
            )));
        }
        require_secure(&discovery.authorization_endpoint)?;
        require_secure(&discovery.token_endpoint)?;
        Ok(discovery)
    }

    /// The address to open in the system browser for this attempt.
    pub async fn authorization_url(&self, attempt: &Attempt) -> Result<String, IdentityError> {
        let discovery = self.discover(&attempt.issuer).await?;
        let mut url = require_secure(&discovery.authorization_endpoint)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &attempt.client_id)
            .append_pair("redirect_uri", &attempt.redirect_uri)
            .append_pair("scope", SCOPES)
            .append_pair("state", &attempt.state)
            .append_pair("nonce", &attempt.nonce)
            .append_pair("code_challenge", &attempt.challenge())
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    /// Checks the address the provider sent the browser back to and
    /// exchanges its code for the ID token.
    pub async fn redeem(&self, attempt: &Attempt, redirect: &str) -> Result<String, IdentityError> {
        let redirect = Url::parse(redirect.trim())
            .map_err(|_| IdentityError::Rejected("the returned address is not valid".to_owned()))?;
        let find = |name: &str| {
            redirect
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        // The state comes first: an address that does not carry this
        // attempt's state says nothing about this attempt, errors included.
        if find("state").as_deref() != Some(attempt.state.as_str()) {
            return Err(IdentityError::Rejected(
                "the answer does not belong to this sign-in".to_owned(),
            ));
        }
        if let Some(error) = find("error") {
            let detail = find("error_description").unwrap_or_default();
            return Err(IdentityError::Rejected(
                format!("the sign-in service answered {error} {detail}")
                    .trim_end()
                    .to_owned(),
            ));
        }
        let code = find("code")
            .ok_or_else(|| IdentityError::Rejected("the answer carries no code".to_owned()))?;

        let discovery = self.discover(&attempt.issuer).await?;
        // A code must come from the provider this attempt went to, or it
        // would be sent to the wrong token endpoint (RFC 9207).
        match find("iss") {
            Some(issuer) if issuer.trim_end_matches('/') == attempt.issuer => {}
            Some(_) => {
                return Err(IdentityError::Rejected(
                    "the answer came from a different sign-in service".to_owned(),
                ));
            }
            None if discovery.authorization_response_iss_parameter_supported => {
                return Err(IdentityError::Rejected(
                    "the answer does not say which sign-in service sent it".to_owned(),
                ));
            }
            None => {}
        }

        let response = self
            .http
            .post(&discovery.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("redirect_uri", attempt.redirect_uri.as_str()),
                ("client_id", attempt.client_id.as_str()),
                ("code_verifier", attempt.verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|error| IdentityError::Provider(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            // The body may describe the refusal, but it is the provider's
            // text about a secret exchange, so only the status is reported.
            return Err(IdentityError::Rejected(format!(
                "the sign-in service refused the code ({status})"
            )));
        }
        let tokens: TokenResponse = response
            .json()
            .await
            .map_err(|error| IdentityError::Provider(error.to_string()))?;
        tokens.id_token.ok_or_else(|| {
            IdentityError::Provider("the sign-in service returned no ID token".to_owned())
        })
    }
}

/// A listener on this machine for the browser's return, on a port the system
/// chose.
#[derive(Debug)]
pub struct Loopback {
    listener: TcpListener,
    redirect_uri: String,
}

impl Loopback {
    pub async fn bind() -> Result<Self, IdentityError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}{CALLBACK_PATH}"),
        })
    }

    /// The redirect address to start the [`Attempt`] with.
    #[must_use]
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Waits for the browser to come back with this attempt's state, answers
    /// it with a page, and returns the address it came back to.
    ///
    /// This waits for as long as it takes. A sign-in may never come back,
    /// for example when the person closes the tab, so the caller decides
    /// when to stop by dropping this future.
    ///
    /// Requests for anything else, such as a favicon, and requests carrying
    /// another state are answered and ignored, so that nothing else on the
    /// machine can end the wait by connecting to the port.
    pub async fn wait_for_redirect(self, attempt: &Attempt) -> Result<String, IdentityError> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            let mut stream = BufReader::new(stream);
            let mut request_line = String::new();
            // A connection that sends nothing must not hold up the wait.
            let read =
                tokio::time::timeout(Duration::from_secs(5), stream.read_line(&mut request_line))
                    .await;
            if !matches!(read, Ok(Ok(_))) {
                continue;
            }
            // "GET /callback?code=...&state=... HTTP/1.1"
            let target = request_line.split_whitespace().nth(1).unwrap_or("/");
            let address = format!("http://127.0.0.1{target}");
            let parsed = Url::parse(&address).ok();
            let is_callback = parsed.as_ref().is_some_and(|url| {
                url.path() == CALLBACK_PATH
                    && url
                        .query_pairs()
                        .any(|(key, value)| key == "state" && value == attempt.state.as_str())
            });
            let failed = parsed
                .as_ref()
                .is_some_and(|url| url.query_pairs().any(|(key, _)| key == "error"));

            let (status, text) = match (is_callback, failed) {
                (true, false) => ("200 OK", "You are signed in. You can close this tab."),
                (true, true) => ("200 OK", "Sign-in was not completed. Return to Dirt."),
                (false, _) => ("404 Not Found", "Nothing here."),
            };
            let body =
                format!("<!doctype html><meta charset=utf-8><title>Dirt</title><p>{text}</p>");
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.get_mut().write_all(response.as_bytes()).await;
            let _ = stream.get_mut().shutdown().await;

            if is_callback {
                // The port is not part of what is checked later, and the
                // request line does not carry it.
                return Ok(address);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// A stand-in identity provider on this machine.
    async fn provider(iss_parameter: bool) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issuer": server.uri(),
                "authorization_endpoint": format!("{}/oauth/authorize", server.uri()),
                "token_endpoint": format!("{}/oauth/token", server.uri()),
                "authorization_response_iss_parameter_supported": iss_parameter,
            })))
            .mount(&server)
            .await;
        server
    }

    fn attempt_for(server: &MockServer) -> Attempt {
        Attempt::new(
            &NonceResponse {
                nonce: "server-nonce".to_owned(),
                expires_in_secs: 600,
                issuer: server.uri(),
                client_id: "dirt-client".to_owned(),
            },
            "http://127.0.0.1:4000/callback",
        )
        .unwrap()
    }

    fn redirect(attempt: &Attempt, extra: &str) -> String {
        format!(
            "http://127.0.0.1:4000/callback?code=the-code&state={}{extra}",
            attempt.state
        )
    }

    #[tokio::test]
    async fn the_browser_is_sent_to_the_provider_with_a_challenge_and_the_nonce() {
        let server = provider(true).await;
        let attempt = attempt_for(&server);

        let address = IdentityClient::new()
            .unwrap()
            .authorization_url(&attempt)
            .await
            .unwrap();

        let url = Url::parse(&address).unwrap();
        assert!(address.starts_with(&format!("{}/oauth/authorize?", server.uri())));
        let get = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
                .unwrap_or_default()
        };
        assert_eq!(get("response_type"), "code");
        assert_eq!(get("client_id"), "dirt-client");
        assert_eq!(get("redirect_uri"), "http://127.0.0.1:4000/callback");
        assert_eq!(get("nonce"), "server-nonce");
        assert_eq!(get("state"), attempt.state);
        assert_eq!(get("code_challenge_method"), "S256");
        assert!(get("scope").split(' ').any(|scope| scope == "openid"));
        // The challenge is the hash of the verifier, and the verifier itself
        // is nowhere in the address.
        assert_eq!(get("code_challenge"), attempt.challenge());
        assert!(!address.contains(&attempt.verifier));
    }

    #[tokio::test]
    async fn a_good_redirect_is_exchanged_for_the_id_token_with_the_verifier() {
        let server = provider(true).await;
        let attempt = attempt_for(&server);
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=the-code"))
            .and(body_string_contains(format!(
                "code_verifier={}",
                attempt.verifier
            )))
            .and(body_string_contains("client_id=dirt-client"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "never-read", "refresh_token": "never-read",
                "id_token": "the.id.token", "token_type": "Bearer",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let iss = format!("&iss={}", server.uri());
        let token = IdentityClient::new()
            .unwrap()
            .redeem(&attempt, &redirect(&attempt, &iss))
            .await
            .unwrap();

        assert_eq!(token, "the.id.token");
    }

    #[tokio::test]
    async fn a_redirect_that_is_not_this_attempts_is_refused_before_any_exchange() {
        let server = provider(true).await;
        let attempt = attempt_for(&server);
        // No token endpoint is mounted: reaching it would fail differently.
        let client = IdentityClient::new().unwrap();
        let iss = format!("&iss={}", server.uri());

        let cases = [
            // Another attempt's state.
            "http://127.0.0.1:4000/callback?code=the-code&state=someone-elses".to_owned(),
            // No state at all.
            "http://127.0.0.1:4000/callback?code=the-code".to_owned(),
            // Another provider's code.
            redirect(&attempt, "&iss=https://evil.example"),
            // A provider that says it names itself, and did not.
            redirect(&attempt, ""),
            // No code.
            format!(
                "http://127.0.0.1:4000/callback?state={}{iss}",
                attempt.state
            ),
            "not an address".to_owned(),
        ];
        for case in cases {
            let error = client.redeem(&attempt, &case).await.unwrap_err();
            assert!(
                matches!(error, IdentityError::Rejected(_)),
                "{case}: {error}"
            );
        }
        assert!(server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.url.path() != "/oauth/token"));
    }

    #[tokio::test]
    async fn a_provider_that_does_not_name_itself_in_redirects_is_accepted_without_iss() {
        let server = provider(false).await;
        let attempt = attempt_for(&server);
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "id_token": "the.id.token" })),
            )
            .mount(&server)
            .await;

        let token = IdentityClient::new()
            .unwrap()
            .redeem(&attempt, &redirect(&attempt, ""))
            .await
            .unwrap();
        assert_eq!(token, "the.id.token");
    }

    #[tokio::test]
    async fn the_providers_refusals_are_reported_as_rejections() {
        let server = provider(true).await;
        let attempt = attempt_for(&server);
        let client = IdentityClient::new().unwrap();

        // The person cancelled in the browser.
        let cancelled = format!(
            "http://127.0.0.1:4000/callback?error=access_denied&error_description=cancelled&state={}",
            attempt.state
        );
        let error = client.redeem(&attempt, &cancelled).await.unwrap_err();
        assert!(error.to_string().contains("access_denied"), "{error}");

        // The token endpoint refuses the code. Its body is not repeated.
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(400).set_body_string("secret detail"))
            .mount(&server)
            .await;
        let iss = format!("&iss={}", server.uri());
        let error = client
            .redeem(&attempt, &redirect(&attempt, &iss))
            .await
            .unwrap_err();
        assert!(matches!(error, IdentityError::Rejected(_)));
        assert!(!error.to_string().contains("secret detail"));
    }

    #[tokio::test]
    async fn a_provider_that_describes_another_issuer_is_refused() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issuer": "https://someone-else.example",
                "authorization_endpoint": format!("{}/oauth/authorize", server.uri()),
                "token_endpoint": format!("{}/oauth/token", server.uri()),
            })))
            .mount(&server)
            .await;
        let attempt = attempt_for(&server);

        let error = IdentityClient::new()
            .unwrap()
            .authorization_url(&attempt)
            .await
            .unwrap_err();
        assert!(matches!(error, IdentityError::Provider(_)), "{error}");
    }

    #[test]
    fn only_https_or_this_machine_is_an_acceptable_provider() {
        let nonce = |issuer: &str| NonceResponse {
            nonce: "n".to_owned(),
            expires_in_secs: 600,
            issuer: issuer.to_owned(),
            client_id: "c".to_owned(),
        };
        for good in [
            "https://clerk.example.com",
            "http://127.0.0.1:9000",
            "http://localhost:9000",
        ] {
            assert!(Attempt::new(&nonce(good), "x").is_ok(), "{good}");
        }
        for bad in [
            "http://clerk.example.com",
            "ftp://clerk.example.com",
            "clerk.example.com",
            "",
        ] {
            assert!(
                matches!(
                    Attempt::new(&nonce(bad), "x"),
                    Err(IdentityError::BadAddress(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn an_attempt_can_be_saved_and_loaded_and_does_not_print_its_secrets() {
        let attempt = Attempt::new(
            &NonceResponse {
                nonce: "n".to_owned(),
                expires_in_secs: 600,
                issuer: "https://clerk.example.com/".to_owned(),
                client_id: "c".to_owned(),
            },
            "dev.dirt.app://callback",
        )
        .unwrap();
        assert_eq!(attempt.issuer, "https://clerk.example.com");

        let saved = serde_json::to_string(&attempt).unwrap();
        let loaded: Attempt = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.verifier, attempt.verifier);
        assert_eq!(loaded.state, attempt.state);

        let printed = format!("{attempt:?}");
        assert!(!printed.contains(&attempt.verifier));
        assert!(!printed.contains(&attempt.state));
    }

    #[tokio::test]
    async fn the_listener_ignores_everything_but_this_attempts_redirect() {
        let loopback = Loopback::bind().await.unwrap();
        let base = loopback.redirect_uri().to_owned();
        let attempt = Attempt::new(
            &NonceResponse {
                nonce: "n".to_owned(),
                expires_in_secs: 600,
                issuer: "https://clerk.example.com".to_owned(),
                client_id: "c".to_owned(),
            },
            base.clone(),
        )
        .unwrap();
        let state = attempt.state.clone();
        let waiting = tokio::spawn(async move { loopback.wait_for_redirect(&attempt).await });

        let http = reqwest::Client::new();
        let root = base.trim_end_matches("/callback").to_owned();
        // A favicon request and a redirect with another state end nothing.
        let favicon = http
            .get(format!("{root}/favicon.ico"))
            .send()
            .await
            .unwrap();
        assert_eq!(favicon.status(), 404);
        let stray = http
            .get(format!("{base}?code=x&state=not-this-one"))
            .send()
            .await
            .unwrap();
        assert_eq!(stray.status(), 404);
        assert!(!waiting.is_finished());

        let page = http
            .get(format!("{base}?code=the-code&state={state}"))
            .send()
            .await
            .unwrap();
        assert_eq!(page.status(), 200);
        assert!(page.text().await.unwrap().contains("signed in"));

        let address = waiting.await.unwrap().unwrap();
        assert!(address.contains("code=the-code"));
        assert!(address.contains(&state));
    }
}
