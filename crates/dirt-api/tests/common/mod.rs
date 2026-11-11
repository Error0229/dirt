//! Shared by the integration tests: a router on a throwaway database, an
//! identity provider that is a key made in the test, and small helpers for
//! calling the server and reading its answers.

#![allow(dead_code)] // each test file uses a different part

use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dirt_api::{build_router, AppState, Config, Keys};
use dirt_proto::api::{
    NonceResponse, PullResponse, PushItem, PushOutcome, PushRequest, PushResponse,
    RegisterDeviceRequest, RegisterDeviceResponse,
};
use dirt_proto::NoteId;
use http_body_util::BodyExt;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

pub const ISSUER: &str = "https://issuer.test";
pub const CLIENT_ID: &str = "dirt-client";
const KEY_ID: &str = "test-key-1";

struct Issuer {
    signing_key: EncodingKey,
    published: JwkSet,
}

fn make_issuer() -> Issuer {
    let private = RsaPrivateKey::new(&mut rand_core::OsRng, 2048).expect("key generation");
    let der = private.to_pkcs1_der().expect("der");
    let public = private.to_public_key();
    let published = serde_json::from_value(json!({
        "keys": [{
            "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KEY_ID,
            "n": URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
        }]
    }))
    .expect("key set");
    Issuer {
        signing_key: EncodingKey::from_rsa_der(der.as_bytes()),
        published,
    }
}

/// The identity provider the server under test trusts. Made once per test
/// program: generating a key is slow in a debug build.
fn issuer() -> &'static Issuer {
    static ISSUER_KEYS: OnceLock<Issuer> = OnceLock::new();
    ISSUER_KEYS.get_or_init(make_issuer)
}

/// The server, on the database `#[sqlx::test]` made for this test.
pub fn app(pool: PgPool) -> Router {
    build_router(AppState {
        pool,
        config: Arc::new(Config {
            database_url: String::new(),
            clerk_issuer: ISSUER.to_owned(),
            clerk_client_id: CLIENT_ID.to_owned(),
            bind_addr: "127.0.0.1:0".parse().unwrap(),
        }),
        keys: Arc::new(Keys::Fixed(issuer().published.clone())),
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// The claims of an ID token for `user` carrying `nonce`.
pub fn claims(user: &str, nonce: &str) -> Value {
    json!({
        "iss": ISSUER, "sub": user, "aud": [CLIENT_ID],
        "exp": now() + 300, "iat": now(), "nonce": nonce,
        "email": format!("{user}@example.com"),
    })
}

/// Signs claims with the trusted key.
pub fn sign(claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KEY_ID.to_owned());
    encode(&header, claims, &issuer().signing_key).unwrap()
}

/// Signs claims with a key the server does not trust, under the trusted
/// key's name.
pub fn sign_with_another_key(claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KEY_ID.to_owned());
    encode(&header, claims, &make_issuer().signing_key).unwrap()
}

/// One answer from the server.
pub struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

impl Answer {
    /// The error code of an error answer.
    pub fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }

    /// The body as a protocol type. Panics, showing the answer, if the call
    /// did not succeed.
    pub fn json<T: DeserializeOwned>(&self) -> T {
        assert!(
            self.status.is_success(),
            "expected success, got {} {}",
            self.status,
            self.body
        );
        serde_json::from_value(self.body.clone()).unwrap()
    }
}

/// Sends one request to the server.
pub async fn call(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Answer {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    Answer {
        status,
        headers,
        body,
    }
}

pub async fn get(app: &Router, path: &str, token: Option<&str>) -> Answer {
    call(app, Method::GET, path, token, None).await
}

pub async fn post(app: &Router, path: &str, token: Option<&str>, body: Value) -> Answer {
    call(app, Method::POST, path, token, Some(body)).await
}

/// A fresh sign-in nonce from the server.
pub async fn nonce(app: &Router) -> String {
    call(app, Method::POST, "/v1/nonces", None, None)
        .await
        .json::<NonceResponse>()
        .nonce
}

/// The body of a registration request.
pub fn registration(id_token: &str, nonce: &str, name: &str) -> Value {
    serde_json::to_value(RegisterDeviceRequest {
        id_token: id_token.to_owned(),
        nonce: nonce.to_owned(),
        name: name.to_owned(),
        public_key: vec![7; 32],
    })
    .unwrap()
}

/// Attempts to sign `user` in on a new device.
pub async fn try_register(app: &Router, user: &str, name: &str) -> Answer {
    let nonce = nonce(app).await;
    let id_token = sign(&claims(user, &nonce));
    post(
        app,
        "/v1/devices",
        None,
        registration(&id_token, &nonce, name),
    )
    .await
}

/// Signs `user` in on a new device.
pub async fn register(app: &Router, user: &str, name: &str) -> RegisterDeviceResponse {
    try_register(app, user, name).await.json()
}

pub fn note_id() -> NoteId {
    NoteId::new()
}

pub const fn item(note: NoteId, base_change: i64, ciphertext: Vec<u8>) -> PushItem {
    PushItem {
        note_id: note,
        base_change,
        key_version: 1,
        format_version: 1,
        ciphertext,
    }
}

pub async fn try_push(app: &Router, token: &str, items: Vec<PushItem>) -> Answer {
    let body = serde_json::to_value(PushRequest { items }).unwrap();
    post(app, "/v1/notes/push", Some(token), body).await
}

/// Pushes items and returns one outcome per item.
pub async fn push(app: &Router, token: &str, items: Vec<PushItem>) -> Vec<PushOutcome> {
    try_push(app, token, items)
        .await
        .json::<PushResponse>()
        .results
        .into_iter()
        .map(|result| result.outcome)
        .collect()
}

/// Pushes one note and returns its outcome.
pub async fn push_one(
    app: &Router,
    token: &str,
    note: NoteId,
    base_change: i64,
    ciphertext: &[u8],
) -> PushOutcome {
    push(
        app,
        token,
        vec![item(note, base_change, ciphertext.to_vec())],
    )
    .await
    .remove(0)
}

pub async fn pull(app: &Router, token: &str, after: i64) -> PullResponse {
    get(app, &format!("/v1/notes/pull?after={after}"), Some(token))
        .await
        .json()
}

/// The account's usage counters as the database holds them:
/// `(change_counter, note_count, bytes_used)`.
pub async fn usage(pool: &PgPool, account: uuid::Uuid) -> (i64, i64, i64) {
    sqlx::query_as("SELECT change_counter, note_count, bytes_used FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
}
