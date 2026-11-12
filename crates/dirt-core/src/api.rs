//! The client for the Dirt server.
//!
//! [`ApiClient`] speaks the protocol in `dirt_proto::api` and turns every
//! failure into an [`ApiError`] with one of four classes, which is all the
//! rest of the app looks at. It reaches the server through the small
//! [`Transport`] trait: [`HttpTransport`] in the apps, and in tests the real
//! server running in the same process.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use dirt_proto::api::{
    DeviceInfo, DeviceList, ErrorBody, NonceResponse, PullResponse, PushRequest, PushResponse,
    RegisterDeviceRequest, RegisterDeviceResponse, PROTOCOL_HEADER, PROTOCOL_VERSION,
};
// What a client needs in order to act on a failure, re-exported so that the
// interface crates do not depend on the protocol crate for it.
pub use dirt_proto::api::{ErrorClass, ErrorCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::Uuid;

/// A future that can be sent between threads, as a trait method returns it.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One request to the server: a method, a path with its query, headers and
/// an optional JSON body.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: &'static str,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// How requests reach the server.
pub trait Transport: Send + Sync {
    /// Sends one request. An error means no answer came back at all.
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>>;
}

/// The transport the apps use: HTTPS to the server's address.
pub struct HttpTransport {
    base_url: String,
    http: reqwest::Client,
}

impl HttpTransport {
    pub fn new(base_url: &str) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| ApiError::Network(error.to_string()))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            http,
        })
    }
}

impl Transport for HttpTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>> {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .map_err(|error| error.to_string())?;
            let mut builder = self
                .http
                .request(method, format!("{}{}", self.base_url, request.path));
            for (name, value) in request.headers {
                builder = builder.header(name, value);
            }
            if let Some(body) = request.body {
                builder = builder.body(body);
            }
            let response = builder.send().await.map_err(|error| error.to_string())?;
            let status = response.status().as_u16();
            let body = response
                .bytes()
                .await
                .map_err(|error| error.to_string())?
                .to_vec();
            Ok(HttpResponse { status, body })
        })
    }
}

/// Why a call to the server failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    /// No answer came back: the device is offline or the server unreachable.
    #[error("could not reach the server: {0}")]
    Network(String),
    /// The server answered with an error of the protocol.
    #[error("the server refused the request ({code:?}): {message}")]
    Server {
        code: ErrorCode,
        message: String,
        retry_after_secs: Option<u32>,
    },
    /// The server answered with something this version cannot read.
    #[error("the server's answer could not be read: {0}")]
    Unreadable(String),
}

impl ApiError {
    /// What to do about the failure. The class is all the app acts on.
    #[must_use]
    pub const fn class(&self) -> ErrorClass {
        match self {
            Self::Network(_) => ErrorClass::TryAgainLater,
            Self::Server { code, .. } => code.class(),
            Self::Unreadable(_) => ErrorClass::UpdateTheApp,
        }
    }

    /// The protocol's code, if the server sent one.
    #[must_use]
    pub const fn code(&self) -> Option<ErrorCode> {
        match self {
            Self::Server { code, .. } => Some(*code),
            _ => None,
        }
    }
}

/// An error answer that did not carry the protocol's error body, as the
/// hosting platform sends when it refuses a request before the server runs.
fn error_from_status(status: u16) -> ApiError {
    let code = match status {
        401 => ErrorCode::Unauthenticated,
        413 => ErrorCode::PayloadTooLarge,
        429 => ErrorCode::RateLimited,
        500..=599 => ErrorCode::ServerError,
        other => return ApiError::Unreadable(format!("unexpected status {other}")),
    };
    ApiError::Server {
        code,
        message: format!("status {status}"),
        retry_after_secs: None,
    }
}

/// A client for one server, with or without a device token.
#[derive(Clone)]
pub struct ApiClient {
    transport: Arc<dyn Transport>,
    token: Option<String>,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("signed_in", &self.token.is_some())
            .finish_non_exhaustive()
    }
}

impl ApiClient {
    /// A client with no device token, for the calls made before sign-in.
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            token: None,
        }
    }

    /// The same client, acting as the device this token belongs to.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    async fn call<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        method: &'static str,
        path: String,
        body: Option<&B>,
    ) -> Result<T, ApiError> {
        let mut headers = vec![(PROTOCOL_HEADER.to_owned(), PROTOCOL_VERSION.to_string())];
        if let Some(token) = &self.token {
            headers.push(("authorization".to_owned(), format!("Bearer {token}")));
        }
        let body = match body {
            Some(body) => {
                headers.push(("content-type".to_owned(), "application/json".to_owned()));
                Some(
                    serde_json::to_vec(body)
                        .map_err(|error| ApiError::Unreadable(error.to_string()))?,
                )
            }
            None => None,
        };
        let response = self
            .transport
            .send(HttpRequest {
                method,
                path,
                headers,
                body,
            })
            .await
            .map_err(ApiError::Network)?;

        if (200..300).contains(&response.status) {
            return serde_json::from_slice(&response.body)
                .map_err(|error| ApiError::Unreadable(error.to_string()));
        }
        match serde_json::from_slice::<ErrorBody>(&response.body) {
            Ok(error) => Err(ApiError::Server {
                code: error.code,
                message: error.message,
                retry_after_secs: error.retry_after_secs,
            }),
            Err(_) => Err(error_from_status(response.status)),
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: String) -> Result<T, ApiError> {
        self.call::<(), T>("GET", path, None).await
    }

    /// A single-use value to carry through the Clerk sign-in.
    pub async fn nonce(&self) -> Result<NonceResponse, ApiError> {
        self.call::<(), _>("POST", "/v1/nonces".to_owned(), None)
            .await
    }

    /// Registers this device with the ID token from the Clerk sign-in.
    pub async fn register(
        &self,
        request: &RegisterDeviceRequest,
    ) -> Result<RegisterDeviceResponse, ApiError> {
        self.call("POST", "/v1/devices".to_owned(), Some(request))
            .await
    }

    /// This device's own entry: whether it is approved yet.
    pub async fn me(&self) -> Result<DeviceInfo, ApiError> {
        self.get("/v1/devices/me".to_owned()).await
    }

    pub async fn devices(&self) -> Result<DeviceList, ApiError> {
        self.get("/v1/devices".to_owned()).await
    }

    /// Removes a device from the account.
    pub async fn revoke(&self, device: Uuid) -> Result<(), ApiError> {
        self.call::<(), serde_json::Value>("DELETE", format!("/v1/devices/{device}"), None)
            .await
            .map(|_| ())
    }

    pub async fn push(&self, request: &PushRequest) -> Result<PushResponse, ApiError> {
        self.call("POST", "/v1/notes/push".to_owned(), Some(request))
            .await
    }

    /// Blobs changed after change number `after`.
    pub async fn pull(&self, after: i64) -> Result<PullResponse, ApiError> {
        self.get(format!("/v1/notes/pull?after={after}")).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// A transport that answers every request with one canned response and
    /// remembers what it was asked.
    struct Canned {
        status: u16,
        body: &'static str,
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl Canned {
        fn client(status: u16, body: &'static str) -> (Arc<Self>, ApiClient) {
            let transport = Arc::new(Self {
                status,
                body,
                seen: Mutex::new(Vec::new()),
            });
            (Arc::clone(&transport), ApiClient::new(transport))
        }
    }

    impl Transport for Canned {
        fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>> {
            self.seen.lock().unwrap().push(request);
            Box::pin(async move {
                Ok(HttpResponse {
                    status: self.status,
                    body: self.body.as_bytes().to_vec(),
                })
            })
        }
    }

    struct Offline;

    impl Transport for Offline {
        fn send(&self, _: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>> {
            Box::pin(async { Err("no route to host".to_owned()) })
        }
    }

    #[tokio::test]
    async fn every_request_names_the_protocol_and_carries_the_token() {
        let (transport, client) =
            Canned::client(200, r#"{"epoch":1,"items":[],"next_after":7,"more":false}"#);
        let page = client.with_token("device-token").pull(7).await.unwrap();
        assert_eq!(page.next_after, 7);

        let seen = transport.seen.lock().unwrap();
        assert_eq!(seen[0].method, "GET");
        assert_eq!(seen[0].path, "/v1/notes/pull?after=7");
        let header = |name: &str| {
            seen[0]
                .headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(header(PROTOCOL_HEADER).as_deref(), Some("1"));
        assert_eq!(
            header("authorization").as_deref(),
            Some("Bearer device-token")
        );
    }

    #[tokio::test]
    async fn a_protocol_error_keeps_its_code_and_class() {
        let (_, client) = Canned::client(
            429,
            r#"{"code":"rate_limited","message":"slow down","retry_after_secs":42}"#,
        );
        let error = client.pull(0).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::RateLimited));
        assert_eq!(error.class(), ErrorClass::TryAgainLater);
        assert!(matches!(
            error,
            ApiError::Server {
                retry_after_secs: Some(42),
                ..
            }
        ));
    }

    #[tokio::test]
    async fn each_class_of_failure_is_recognised() {
        for (status, body, class) in [
            (
                401,
                r#"{"code":"device_revoked","message":"removed"}"#,
                ErrorClass::StopUntilSignedIn,
            ),
            (
                426,
                r#"{"code":"unsupported_protocol","message":"update"}"#,
                ErrorClass::UpdateTheApp,
            ),
            (
                507,
                r#"{"code":"quota_exceeded","message":"full"}"#,
                ErrorClass::ProblemWithOneNote,
            ),
            // Answers from the hosting platform carry no protocol body.
            (502, "<html>Bad Gateway</html>", ErrorClass::TryAgainLater),
            (413, "Request Entity Too Large", ErrorClass::UpdateTheApp),
            (418, "I'm a teapot", ErrorClass::UpdateTheApp),
        ] {
            let (_, client) = Canned::client(status, body);
            assert_eq!(client.pull(0).await.unwrap_err().class(), class, "{status}");
        }
        // A success whose body is not what the protocol says.
        let (_, client) = Canned::client(200, "not json");
        assert_eq!(
            client.pull(0).await.unwrap_err().class(),
            ErrorClass::UpdateTheApp
        );
    }

    #[tokio::test]
    async fn no_answer_at_all_means_try_again_later() {
        let error = ApiClient::new(Arc::new(Offline)).pull(0).await.unwrap_err();
        assert!(matches!(error, ApiError::Network(_)));
        assert_eq!(error.class(), ErrorClass::TryAgainLater);
    }

    #[test]
    fn the_token_never_appears_in_debug_output() {
        let client = ApiClient::new(Arc::new(Offline)).with_token("secret-device-token");
        assert!(!format!("{client:?}").contains("secret-device-token"));
    }
}
