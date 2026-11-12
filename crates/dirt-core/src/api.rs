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
    ChangeKey, DeviceKeys, DeviceList, DownloadLink, EnrolmentMessage, EnrolmentMessages,
    ErrorBody, FileReference, FileReferences, FileResults, KeyChanged, MyDevice, NonceResponse,
    OldKeyNotes, PostEnrolmentMessage, PullResponse, PurgeRequest, PurgeResponse, PushRequest,
    PushResponse, PutRecovery, RecoveryClaim, RegisterDeviceRequest, RegisterDeviceResponse,
    ResetResponse, UploadLink, UploadRequest, WrappedKey, PROTOCOL_HEADER, PROTOCOL_VERSION,
};
// What a client needs in order to act on a failure, re-exported so that the
// interface crates do not depend on the protocol crate for it.
pub use dirt_proto::api::{DeviceInfo, DeviceStatus, ErrorClass, ErrorCode};
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

/// One request to the file storage, at an address the server signed.
///
/// A file's bytes never pass through the server. The server gives a link,
/// and the device sends to the storage or fetches from it directly.
#[derive(Debug, Clone)]
pub struct StorageRequest {
    /// `PUT` to send a file, `GET` to fetch one.
    pub method: &'static str,
    /// The whole address, as the server gave it.
    pub url: String,
    pub body: Option<Vec<u8>>,
}

/// How requests reach the server.
pub trait Transport: Send + Sync {
    /// Sends one request. An error means no answer came back at all.
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>>;

    /// Sends one request to the file storage. A transport that has no way
    /// to reach it says so, and files then wait on the device.
    fn send_to_storage(
        &self,
        _request: StorageRequest,
    ) -> BoxFuture<'_, Result<HttpResponse, String>> {
        Box::pin(async { Err("this transport cannot reach the file storage".to_owned()) })
    }
}

/// How long one file may take to send or fetch. Ten megabytes on a slow
/// connection is minutes, where a request to the server is seconds.
const STORAGE_TIMEOUT: Duration = Duration::from_secs(600);

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

    fn send_to_storage(
        &self,
        request: StorageRequest,
    ) -> BoxFuture<'_, Result<HttpResponse, String>> {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .map_err(|error| error.to_string())?;
            // The address is used exactly as it was signed. Nothing of
            // this device's is added to it: no token, no header.
            let mut builder = self
                .http
                .request(method, &request.url)
                .timeout(STORAGE_TIMEOUT);
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

    /// This device's own entry, with what it needs to know about its
    /// account: whether it is approved yet, who could approve it, whether a
    /// recovery copy exists.
    pub async fn me(&self) -> Result<MyDevice, ApiError> {
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

    /// Asks the server to purge deleted notes. Each is purged only if the
    /// server still holds the version named for it.
    pub async fn purge(&self, request: &PurgeRequest) -> Result<PurgeResponse, ApiError> {
        self.call("POST", "/v1/notes/purge".to_owned(), Some(request))
            .await
    }

    // ---- Files ----

    /// A link to send one sealed file of exactly `size` bytes.
    pub async fn upload_link(&self, file_id: Uuid, size: i64) -> Result<UploadLink, ApiError> {
        self.call(
            "POST",
            "/v1/files".to_owned(),
            Some(&UploadRequest { file_id, size }),
        )
        .await
    }

    /// A link to fetch one sealed file.
    pub async fn download_link(&self, file_id: Uuid) -> Result<DownloadLink, ApiError> {
        self.get(format!("/v1/files/{file_id}")).await
    }

    /// Tells the server that these notes refer to these files.
    pub async fn confirm_files(&self, items: Vec<FileReference>) -> Result<FileResults, ApiError> {
        self.call(
            "POST",
            "/v1/files/confirm".to_owned(),
            Some(&FileReferences { items }),
        )
        .await
    }

    /// Tells the server that these notes no longer refer to these files.
    pub async fn release_files(&self, items: Vec<FileReference>) -> Result<FileResults, ApiError> {
        self.call(
            "POST",
            "/v1/files/release".to_owned(),
            Some(&FileReferences { items }),
        )
        .await
    }

    /// Sends a sealed file to the storage through a link from
    /// [`Self::upload_link`]. The device's token is not sent: the link is
    /// all the storage needs, and all it is given.
    pub async fn put_file(&self, url: String, sealed: Vec<u8>) -> Result<(), ApiError> {
        let response = self
            .transport
            .send_to_storage(StorageRequest {
                method: "PUT",
                url,
                body: Some(sealed),
            })
            .await
            .map_err(ApiError::Network)?;
        if (200..300).contains(&response.status) {
            Ok(())
        } else {
            Err(ApiError::Network(format!(
                "the file storage answered {}",
                response.status
            )))
        }
    }

    /// Fetches a sealed file from the storage through a link from
    /// [`Self::download_link`]. `None` if the storage has no such file.
    pub async fn get_file(&self, url: String) -> Result<Option<Vec<u8>>, ApiError> {
        let response = self
            .transport
            .send_to_storage(StorageRequest {
                method: "GET",
                url,
                body: None,
            })
            .await
            .map_err(ApiError::Network)?;
        match response.status {
            status if (200..300).contains(&status) => Ok(Some(response.body)),
            404 => Ok(None),
            status => Err(ApiError::Network(format!(
                "the file storage answered {status}"
            ))),
        }
    }

    /// Blobs changed after change number `after`.
    pub async fn pull(&self, after: i64) -> Result<PullResponse, ApiError> {
        self.get(format!("/v1/notes/pull?after={after}")).await
    }

    // ---- Changing the account's key ----

    /// Changes the account's key: stores the new key wrapped for every
    /// approved device and moves the account to the next version.
    pub async fn change_key(&self, request: &ChangeKey) -> Result<KeyChanged, ApiError> {
        self.call("POST", "/v1/account/key".to_owned(), Some(request))
            .await
    }

    /// This device's wrapped copies of the account keys after version
    /// `after`, oldest first.
    pub async fn account_keys(&self, after: i32) -> Result<Vec<WrappedKey>, ApiError> {
        let copies: DeviceKeys = self.get(format!("/v1/account/keys?after={after}")).await?;
        Ok(copies.keys)
    }

    /// A page of the notes the server still holds under an older key:
    /// those whose last change is numbered after `after`.
    pub async fn old_key_notes(&self, after: i64) -> Result<OldKeyNotes, ApiError> {
        self.get(format!("/v1/account/key/notes?after={after}"))
            .await
    }

    // ---- The approval exchange ----

    /// Adds a message to the exchange of the waiting device `device`. With
    /// `restart`, which only that device may ask for, the server first
    /// drops the exchange so far.
    pub async fn post_enrolment_message(
        &self,
        device: Uuid,
        body: Vec<u8>,
        restart: bool,
    ) -> Result<EnrolmentMessage, ApiError> {
        self.call(
            "POST",
            format!("/v1/enrolments/{device}/messages"),
            Some(&PostEnrolmentMessage { body, restart }),
        )
        .await
    }

    /// The messages of an exchange numbered above `after`.
    pub async fn enrolment_messages(
        &self,
        device: Uuid,
        after: i64,
    ) -> Result<Vec<EnrolmentMessage>, ApiError> {
        let answer: EnrolmentMessages = self
            .get(format!("/v1/enrolments/{device}/messages?after={after}"))
            .await?;
        Ok(answer.messages)
    }

    /// Leaves the account key, encrypted for the waiting device.
    pub async fn put_enrolment_key(&self, device: Uuid, key: &WrappedKey) -> Result<(), ApiError> {
        self.call::<_, serde_json::Value>("PUT", format!("/v1/enrolments/{device}/key"), Some(key))
            .await
            .map(|_| ())
    }

    /// The encrypted key left for this waiting device, or `None` if no
    /// approved device has left one yet.
    pub async fn enrolment_key(&self, device: Uuid) -> Result<Option<WrappedKey>, ApiError> {
        not_found_is_none(self.get(format!("/v1/enrolments/{device}/key")).await)
    }

    /// Tells the server this waiting device has the key. The server then
    /// approves it.
    pub async fn complete_enrolment(&self, device: Uuid) -> Result<(), ApiError> {
        self.call::<(), serde_json::Value>(
            "POST",
            format!("/v1/enrolments/{device}/complete"),
            None,
        )
        .await
        .map(|_| ())
    }

    // ---- The recovery copy ----

    pub async fn put_recovery(&self, copy: &PutRecovery) -> Result<(), ApiError> {
        self.call::<_, serde_json::Value>("PUT", "/v1/recovery".to_owned(), Some(copy))
            .await
            .map(|_| ())
    }

    /// The account's recovery copy, or `None` if it has none.
    pub async fn recovery(&self) -> Result<Option<WrappedKey>, ApiError> {
        not_found_is_none(self.get("/v1/recovery".to_owned()).await)
    }

    pub async fn delete_recovery(&self) -> Result<(), ApiError> {
        self.call::<(), serde_json::Value>("DELETE", "/v1/recovery".to_owned(), None)
            .await
            .map(|_| ())
    }

    /// Presents the claim code derived from the recovery phrase. The server
    /// approves this device if the code is the account's.
    pub async fn claim_recovery(&self, claim_code: Vec<u8>) -> Result<(), ApiError> {
        self.call::<_, serde_json::Value>(
            "POST",
            "/v1/recovery/claim".to_owned(),
            Some(&RecoveryClaim { claim_code }),
        )
        .await
        .map(|_| ())
    }

    // ---- The account ----

    pub async fn delete_account(&self) -> Result<(), ApiError> {
        self.call::<(), serde_json::Value>("DELETE", "/v1/account".to_owned(), None)
            .await
            .map(|_| ())
    }

    /// Resets the account, or asks for it to be reset. The answer says
    /// which happened.
    pub async fn reset_account(&self) -> Result<ResetResponse, ApiError> {
        self.call::<(), _>("POST", "/v1/account/reset".to_owned(), None)
            .await
    }

    /// Refuses a reset that a waiting device asked for.
    pub async fn cancel_reset(&self) -> Result<(), ApiError> {
        self.call::<(), serde_json::Value>("DELETE", "/v1/account/reset".to_owned(), None)
            .await
            .map(|_| ())
    }
}

/// Turns the server's "nothing here" into `None`, for things that may
/// simply not exist yet.
fn not_found_is_none<T>(result: Result<T, ApiError>) -> Result<Option<T>, ApiError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code() == Some(ErrorCode::NotFound) => Ok(None),
        Err(error) => Err(error),
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

    /// A file storage that answers every request with one status, and
    /// remembers what it was asked.
    struct Storage {
        status: u16,
        seen: Mutex<Vec<StorageRequest>>,
    }

    impl Storage {
        fn client(status: u16) -> (Arc<Self>, ApiClient) {
            let transport = Arc::new(Self {
                status,
                seen: Mutex::new(Vec::new()),
            });
            let client = ApiClient::new(Arc::clone(&transport) as Arc<dyn Transport>)
                .with_token("the device's token".to_owned());
            (transport, client)
        }
    }

    impl Transport for Storage {
        fn send(&self, _: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>> {
            Box::pin(async { Err("this is not the server".to_owned()) })
        }

        fn send_to_storage(
            &self,
            request: StorageRequest,
        ) -> BoxFuture<'_, Result<HttpResponse, String>> {
            self.seen.lock().unwrap().push(request);
            Box::pin(async move {
                Ok(HttpResponse {
                    status: self.status,
                    body: b"sealed bytes".to_vec(),
                })
            })
        }
    }

    /// A file goes to the address the server signed and nowhere else, as
    /// it was sealed. The request has no place for the device's token:
    /// the storage is another party, and the link is all it is given.
    #[tokio::test]
    async fn a_file_is_sent_and_fetched_at_the_signed_address() {
        let (storage, client) = Storage::client(200);
        let link = "https://storage.example/bucket/account/file?X-Amz-Signature=abc";

        client
            .put_file(link.to_owned(), b"sealed".to_vec())
            .await
            .unwrap();
        let fetched = client.get_file(link.to_owned()).await.unwrap();

        assert_eq!(fetched.as_deref(), Some(&b"sealed bytes"[..]));
        let seen = storage.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!((seen[0].method, seen[0].url.as_str()), ("PUT", link));
        assert_eq!(seen[0].body.as_deref(), Some(&b"sealed"[..]));
        assert_eq!((seen[1].method, seen[1].url.as_str()), ("GET", link));
        assert_eq!(seen[1].body, None);
    }

    #[tokio::test]
    async fn a_storage_that_refuses_is_a_failure_to_try_again() {
        // A file that is not there is an answer, not a failure.
        let (_storage, client) = Storage::client(404);
        assert_eq!(
            client.get_file("https://s/x".to_owned()).await.unwrap(),
            None
        );
        // An upload to a place that is not there is a failure.
        let error = client
            .put_file("https://s/x".to_owned(), Vec::new())
            .await
            .unwrap_err();
        assert_eq!(error.class(), ErrorClass::TryAgainLater);

        for status in [403, 500, 503] {
            let (_storage, client) = Storage::client(status);
            let error = client.get_file("https://s/x".to_owned()).await.unwrap_err();
            assert_eq!(error.class(), ErrorClass::TryAgainLater, "{status}");
            let error = client
                .put_file("https://s/x".to_owned(), Vec::new())
                .await
                .unwrap_err();
            assert_eq!(error.class(), ErrorClass::TryAgainLater, "{status}");
        }
    }

    /// A transport written before files, or one with no way to the
    /// storage, says so. Files then wait.
    #[tokio::test]
    async fn a_transport_with_no_way_to_the_storage_says_so() {
        let (_transport, client) = Canned::client(200, "{}");
        let error = client
            .put_file("https://s/x".to_owned(), Vec::new())
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::Network(_)), "{error}");
        assert!(client.get_file("https://s/x".to_owned()).await.is_err());
    }

    #[tokio::test]
    async fn the_calls_for_files_go_where_the_protocol_says() {
        let file = Uuid::from_u128(7);
        let note: dirt_proto::NoteId = "0192d3a8-0000-7000-8000-000000000001".parse().unwrap();
        let (transport, client) = Canned::client(
            200,
            r#"{"url":"https://s/x","size":9,"expires_in_secs":300}"#,
        );
        client.upload_link(file, 9).await.unwrap();
        client.download_link(file).await.unwrap();
        let (results, results_client) = Canned::client(200, r#"{"results":[]}"#);
        let reference = || {
            vec![FileReference {
                file_id: file,
                note_id: note,
            }]
        };
        results_client.confirm_files(reference()).await.unwrap();
        results_client.release_files(reference()).await.unwrap();

        let asked = |transport: &Canned| -> Vec<(&'static str, String, Option<String>)> {
            transport
                .seen
                .lock()
                .unwrap()
                .iter()
                .map(|request| {
                    (
                        request.method,
                        request.path.clone(),
                        request
                            .body
                            .as_ref()
                            .map(|body| String::from_utf8(body.clone()).unwrap()),
                    )
                })
                .collect()
        };
        let body = format!(r#"{{"items":[{{"file_id":"{file}","note_id":"{note}"}}]}}"#);
        assert_eq!(
            asked(&transport),
            [
                (
                    "POST",
                    "/v1/files".to_owned(),
                    Some(format!(r#"{{"file_id":"{file}","size":9}}"#))
                ),
                ("GET", format!("/v1/files/{file}"), None),
            ]
        );
        assert_eq!(
            asked(&results),
            [
                ("POST", "/v1/files/confirm".to_owned(), Some(body.clone())),
                ("POST", "/v1/files/release".to_owned(), Some(body)),
            ]
        );
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
