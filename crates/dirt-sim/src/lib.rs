//! Real Dirt clients against the real server, in one process.
//!
//! A [`Server`] is the actual `dirt-api` router on a Postgres database, with
//! an identity provider that is a key made on the spot. A [`Device`] is an
//! actual client core: a local store on a temporary file and a sync engine
//! that reaches the server through [`InProcess`], a transport that calls the
//! router directly and can be told to misbehave.
//!
//! A [`Client`] is a whole client core as an app holds it: a
//! [`Session`](dirt_core::session::Session) that signs in through a
//! [`provider`], a stand-in for the identity provider that speaks the real
//! protocol over HTTP on this machine.
//!
//! Handing the account key to a second device arrives in step 6 of the
//! redesign. Until then a test approves the device in the database and gives
//! it the first device's key (amendment C3).

pub mod provider;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dirt_api::{build_router, AppState, Config, Keys};
use dirt_core::api::{ApiClient, BoxFuture, HttpRequest, HttpResponse, Transport};
use dirt_core::crypto::{AccountKey, DeviceKeyPair};
use dirt_core::secrets::{DeviceSecrets, MemorySecrets, SecretStore};
use dirt_core::session::{Session, SessionConfig, SessionError, SessionState};
use dirt_core::sync::engine::{SyncEngine, SyncError, SyncReport};
use dirt_core::sync::worker::Timing;
use dirt_core::Store;
use dirt_proto::api::{DeviceStatus, RegisterDeviceRequest, RegisterDeviceResponse};
use http_body_util::BodyExt;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ISSUER: &str = "https://issuer.test";
const CLIENT_ID: &str = "dirt-client";
const KEY_ID: &str = "test-key-1";

struct Issuer {
    signing_key: EncodingKey,
    published: JwkSet,
}

/// The identity provider every simulated server trusts. Made once per test
/// program: generating a key is slow in a debug build.
fn issuer() -> &'static Issuer {
    static ISSUER_KEYS: OnceLock<Issuer> = OnceLock::new();
    ISSUER_KEYS.get_or_init(|| {
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
    })
}

fn id_token(issuer_url: &str, user: &str, nonce: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KEY_ID.to_owned());
    let claims = json!({
        "iss": issuer_url, "sub": user, "aud": [CLIENT_ID],
        "exp": now + 300, "iat": now, "nonce": nonce,
    });
    encode(&header, &claims, &issuer().signing_key).expect("signing")
}

/// The real server, on the database a test was given.
#[derive(Clone)]
pub struct Server {
    router: Router,
    pub pool: PgPool,
    /// The address of the identity provider this server trusts.
    issuer: String,
}

impl Server {
    /// A server whose identity provider is a name only. Devices made with
    /// [`Device::sign_in`] are given tokens signed in its name.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self::trusting(pool, ISSUER.to_owned())
    }

    /// A server with a running identity provider, for clients that sign in
    /// the way an app does.
    pub async fn with_sign_in(pool: PgPool) -> Self {
        Self::trusting(pool, provider::start().await)
    }

    fn trusting(pool: PgPool, issuer_url: String) -> Self {
        let router = build_router(AppState {
            pool: pool.clone(),
            config: Arc::new(Config {
                database_url: String::new(),
                clerk_issuer: issuer_url.clone(),
                clerk_client_id: CLIENT_ID.to_owned(),
                bind_addr: "127.0.0.1:0".parse().expect("address"),
            }),
            keys: Arc::new(Keys::Fixed(issuer().published.clone())),
        });
        Self {
            router,
            pool,
            issuer: issuer_url,
        }
    }

    /// The address of the identity provider this server trusts.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// A transport to this server for one client.
    #[must_use]
    pub fn transport(&self) -> Arc<InProcess> {
        Arc::new(InProcess {
            router: self.router.clone(),
            faults: Mutex::new(Vec::new()),
            offline: AtomicBool::new(false),
            requests: AtomicUsize::new(0),
        })
    }

    /// The status the server holds for a device.
    pub async fn device_status(&self, device: Uuid) -> String {
        sqlx::query_scalar("SELECT status FROM devices WHERE id = $1")
            .bind(device)
            .fetch_one(&self.pool)
            .await
            .expect("device status")
    }

    /// How many note blobs the server holds.
    pub async fn blob_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM notes WHERE ciphertext IS NOT NULL")
            .fetch_one(&self.pool)
            .await
            .expect("blob count")
    }

    /// Marks a pending device as approved, as an approved device will do in
    /// step 6 by handing over the account key.
    pub async fn approve(&self, device: Uuid) {
        sqlx::query("UPDATE devices SET status = 'approved' WHERE id = $1")
            .bind(device)
            .execute(&self.pool)
            .await
            .expect("approve");
    }

    /// Makes a device's token look unused for longer than the server
    /// allows, so that its next request is refused as expired.
    pub async fn expire(&self, device: Uuid) {
        sqlx::query("UPDATE devices SET last_seen = now() - interval '200 days' WHERE id = $1")
            .bind(device)
            .execute(&self.pool)
            .await
            .expect("expire");
    }
}

/// Something a transport can be told to do to the next matching request.
enum Fault {
    /// Let the server handle the request, then lose its answer.
    LoseAnswer,
    /// Run this before the request reaches the server.
    Before(Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>),
    /// Rewrite the server's answer before the client sees it.
    Rewrite(Box<dyn FnOnce(&mut serde_json::Value) + Send>),
}

/// A transport that calls the server's router directly.
pub struct InProcess {
    router: Router,
    /// Faults waiting for a request whose path starts with the given text.
    faults: Mutex<Vec<(String, Fault)>>,
    /// While set, no request reaches the server.
    offline: AtomicBool,
    /// How many requests were attempted, whether or not they got through.
    requests: AtomicUsize,
}

impl InProcess {
    /// A fault is named by the start of a path, or by a method and the
    /// start of a path, as in `POST /v1/notes/`.
    fn take_fault(&self, method: &str, path: &str) -> Option<Fault> {
        let with_method = format!("{method} {path}");
        let mut faults = self.faults.lock().expect("faults");
        let index = faults.iter().position(|(prefix, _)| {
            path.starts_with(prefix.as_str()) || with_method.starts_with(prefix.as_str())
        })?;
        Some(faults.remove(index).1)
    }

    fn add_fault(&self, path_prefix: &str, fault: Fault) {
        self.faults
            .lock()
            .expect("faults")
            .push((path_prefix.to_owned(), fault));
    }

    /// Cuts the client off from the server, or reconnects it.
    pub fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::SeqCst);
    }

    /// How many requests the client has attempted.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// Loses the server's answer to the next request whose path starts with
    /// `path_prefix`. The server still handles the request.
    pub fn lose_next_answer(&self, path_prefix: &str) {
        self.add_fault(path_prefix, Fault::LoseAnswer);
    }

    /// Rewrites the server's answer to the next request whose path starts
    /// with `path_prefix`, as a dishonest server could.
    pub fn rewrite_next_answer<F>(&self, path_prefix: &str, rewrite: F)
    where
        F: FnOnce(&mut serde_json::Value) + Send + 'static,
    {
        self.add_fault(path_prefix, Fault::Rewrite(Box::new(rewrite)));
    }

    /// Runs `action` just before the next request whose path starts with
    /// `path_prefix` reaches the server.
    pub fn before_next<F>(&self, path_prefix: &str, action: F)
    where
        F: FnOnce() -> BoxFuture<'static, ()> + Send + 'static,
    {
        self.add_fault(path_prefix, Fault::Before(Box::new(action)));
    }
}

impl Transport for InProcess {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>> {
        Box::pin(async move {
            self.requests.fetch_add(1, Ordering::SeqCst);
            if self.offline.load(Ordering::SeqCst) {
                return Err("there is no network".to_owned());
            }
            let fault = self.take_fault(request.method, &request.path);
            let fault = match fault {
                Some(Fault::Before(action)) => {
                    action().await;
                    None
                }
                other => other,
            };

            let mut builder = Request::builder().method(request.method).uri(&request.path);
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            let body = request.body.map_or_else(Body::empty, Body::from);
            let response = self
                .router
                .clone()
                .oneshot(builder.body(body).map_err(|error| error.to_string())?)
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status().as_u16();
            let mut body = response
                .into_body()
                .collect()
                .await
                .map_err(|error| error.to_string())?
                .to_bytes()
                .to_vec();

            match fault {
                Some(Fault::LoseAnswer) => return Err("the connection was lost".to_owned()),
                Some(Fault::Rewrite(rewrite)) => {
                    let mut json = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
                    rewrite(&mut json);
                    body = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
                }
                Some(Fault::Before(_)) | None => {}
            }
            Ok(HttpResponse { status, body })
        })
    }
}

/// One simulated device: a real client core with its own local store.
pub struct Device {
    pub store: Store,
    pub registration: RegisterDeviceResponse,
    pub key: AccountKey,
    public_key: Vec<u8>,
    transport: Arc<InProcess>,
    engine: SyncEngine,
    // Dropped last, after the store has closed its file.
    _folder: tempfile::TempDir,
}

impl Device {
    /// Signs `user` in on a new device. The first device of an account makes
    /// the account key. A later one is approved by the test and given `key`,
    /// which must be the first device's.
    pub async fn sign_in(
        server: &Server,
        user: &str,
        name: &str,
        key: Option<&AccountKey>,
    ) -> Self {
        let transport = server.transport();
        let api = ApiClient::new(Arc::clone(&transport) as Arc<dyn Transport>);
        let public_key = DeviceKeyPair::generate().public_bytes().to_vec();
        let answer = api.nonce().await.expect("nonce");
        let (issuer_url, nonce) = (answer.issuer, answer.nonce);
        let registration = api
            .register(&RegisterDeviceRequest {
                id_token: id_token(&issuer_url, user, &nonce),
                nonce,
                name: name.to_owned(),
                public_key: public_key.clone(),
                previous_token: None,
            })
            .await
            .expect("register");

        let key = match (registration.first_device, key) {
            (true, None) => AccountKey::generate(),
            (false, Some(key)) => {
                assert_eq!(registration.status, DeviceStatus::Pending);
                server.approve(registration.device_id).await;
                key.clone()
            }
            (true, Some(_)) => panic!("the first device of an account makes its own key"),
            (false, None) => panic!("a later device needs the account's key"),
        };

        let folder = tempfile::tempdir().expect("temporary folder");
        let store = Store::open(folder.path().join("notes.db")).expect("store");
        Self::assemble(folder, store, registration, key, public_key, transport)
    }

    fn assemble(
        folder: tempfile::TempDir,
        store: Store,
        registration: RegisterDeviceResponse,
        key: AccountKey,
        public_key: Vec<u8>,
        transport: Arc<InProcess>,
    ) -> Self {
        let api = ApiClient::new(Arc::clone(&transport) as Arc<dyn Transport>)
            .with_token(registration.token.clone());
        let engine = SyncEngine::new(store.clone(), api, registration.account_id, key.clone());
        Self {
            store,
            registration,
            key,
            public_key,
            transport,
            engine,
            _folder: folder,
        }
    }

    /// Signs `user` in again on this device, as it must once its token has
    /// expired. The device keeps its local store, its keys and, if the
    /// server recognises it, its place in the account.
    pub async fn sign_in_again(self, user: &str) -> Self {
        let Self {
            store,
            registration: old,
            key,
            public_key,
            transport,
            engine,
            _folder: folder,
        } = self;
        drop(engine);
        let api = ApiClient::new(Arc::clone(&transport) as Arc<dyn Transport>);
        let answer = api.nonce().await.expect("nonce");
        let (issuer_url, nonce) = (answer.issuer, answer.nonce);
        let registration = api
            .register(&RegisterDeviceRequest {
                id_token: id_token(&issuer_url, user, &nonce),
                nonce,
                name: "the same device".to_owned(),
                public_key: public_key.clone(),
                previous_token: Some(old.token),
            })
            .await
            .expect("register");
        Self::assemble(folder, store, registration, key, public_key, transport)
    }

    /// The same device with its local database replaced by `database`, a
    /// copy of the file taken earlier: what restoring a backup does.
    #[must_use]
    pub fn restored_from(self, database: &[u8]) -> Self {
        let Self {
            store,
            registration,
            key,
            public_key,
            transport,
            engine,
            _folder: folder,
        } = self;
        let path = store.path().to_owned();
        // Both handles must go before the file can be replaced.
        drop(engine);
        drop(store);
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        std::fs::write(&path, database).expect("restore the database file");
        let store = Store::open(&path).expect("reopen the store");
        Self::assemble(folder, store, registration, key, public_key, transport)
    }

    /// Closes the device's local database, returns a copy of its file, and
    /// opens it again: what taking a backup does. Closing first folds the
    /// write-ahead log into the file, so the copy is complete on its own.
    #[must_use]
    pub fn snapshot(self) -> (Self, Vec<u8>) {
        let path = self.store.path().to_owned();
        let Self {
            store,
            registration,
            key,
            public_key,
            transport,
            engine,
            _folder: folder,
        } = self;
        drop(engine);
        drop(store);
        let copy = std::fs::read(&path).expect("read the database file");
        let store = Store::open(&path).expect("reopen the store");
        (
            Self::assemble(folder, store, registration, key, public_key, transport),
            copy,
        )
    }

    /// Runs one sync cycle.
    pub async fn sync(&self) -> Result<SyncReport, SyncError> {
        self.engine.run_once().await
    }

    /// Runs sync cycles until one ends with nothing unsent and nothing
    /// refused as stale. Panics if a cycle fails.
    pub async fn sync_until_settled(&self) -> SyncReport {
        for _ in 0..10 {
            let report = self.sync().await.expect("sync");
            if report.unsent == 0 && report.stale == 0 {
                return report;
            }
        }
        panic!("the device did not settle in ten cycles");
    }

    /// The live notes, as `(id, text)` sorted by ID, for comparing devices.
    pub async fn notes(&self) -> Vec<(String, String)> {
        let mut notes: Vec<(String, String)> = self
            .store
            .list_notes(100_000, 0)
            .await
            .expect("list")
            .into_iter()
            .map(|note| (note.id.to_string(), note.content))
            .collect();
        notes.sort();
        notes
    }

    /// Loses the server's answer to this device's next request whose path
    /// starts with `path_prefix`. The server still handles the request.
    pub fn lose_next_answer(&self, path_prefix: &str) {
        self.transport.add_fault(path_prefix, Fault::LoseAnswer);
    }

    /// Runs `action` just before this device's next request whose path
    /// starts with `path_prefix` reaches the server.
    pub fn before_next<F>(&self, path_prefix: &str, action: F)
    where
        F: FnOnce() -> BoxFuture<'static, ()> + Send + 'static,
    {
        self.transport
            .add_fault(path_prefix, Fault::Before(Box::new(action)));
    }

    /// Rewrites the server's answer to this device's next request whose path
    /// starts with `path_prefix`, as a dishonest server could.
    pub fn rewrite_next_answer<F>(&self, path_prefix: &str, rewrite: F)
    where
        F: FnOnce(&mut serde_json::Value) + Send + 'static,
    {
        self.transport
            .add_fault(path_prefix, Fault::Rewrite(Box::new(rewrite)));
    }
}

/// Pauses short enough for tests, in the proportions the apps use.
#[must_use]
pub const fn fast_timing() -> Timing {
    Timing {
        debounce: Duration::from_millis(40),
        debounce_limit: Duration::from_millis(300),
        return_gap: Duration::from_secs(30),
        first_retry: Duration::from_millis(60),
        longest_retry: Duration::from_millis(240),
        // As the apps have it. A test that wants a purge shortens it.
        purge_after: dirt_core::sync::worker::default_purge_after(),
    }
}

/// One simulated app: a whole client core behind a [`Session`], with its own
/// data folder and its own secure storage.
pub struct Client {
    pub session: Session,
    pub transport: Arc<InProcess>,
    pub secrets: Arc<MemorySecrets>,
    folder: Arc<tempfile::TempDir>,
    config: SessionConfig,
}

impl Client {
    /// A new, signed-out app that reaches `server`. `background_sync` says
    /// whether it runs the sync worker, as an app that stays open does.
    #[must_use]
    pub fn new(server: &Server, name: &str, background_sync: bool) -> Self {
        let folder = Arc::new(tempfile::tempdir().expect("temporary folder"));
        let config = SessionConfig {
            data_folder: folder.path().to_owned(),
            device_name: name.to_owned(),
            background_sync,
            persist_attempts: false,
            timing: fast_timing(),
        };
        Self::open(
            config,
            server.transport(),
            Arc::new(MemorySecrets::new()),
            folder,
        )
    }

    fn open(
        config: SessionConfig,
        transport: Arc<InProcess>,
        secrets: Arc<MemorySecrets>,
        folder: Arc<tempfile::TempDir>,
    ) -> Self {
        let session = Session::open(
            config.clone(),
            Arc::clone(&transport) as Arc<dyn Transport>,
            Arc::clone(&secrets) as Arc<dyn SecretStore>,
        )
        .expect("open the session");
        Self {
            session,
            transport,
            secrets,
            folder,
            config,
        }
    }

    /// The same app after it was closed and started again: the same folder
    /// and secure storage, with `change` applied to its configuration.
    #[must_use]
    pub fn restarted(self, change: impl FnOnce(&mut SessionConfig)) -> Self {
        let Self {
            session,
            transport,
            secrets,
            folder,
            mut config,
        } = self;
        drop(session);
        change(&mut config);
        Self::open(config, transport, secrets, folder)
    }

    /// Another program on the same device, running at the same time: the
    /// CLI beside the desktop app. It shares the data folder and the secure
    /// storage, and has its own connection to the server.
    #[must_use]
    pub fn second_program(&self, server: &Server) -> Self {
        Self::open(
            self.config.clone(),
            server.transport(),
            Arc::clone(&self.secrets),
            Arc::clone(&self.folder),
        )
    }

    /// The app's data folder.
    #[must_use]
    pub fn folder(&self) -> &Path {
        self.folder.path()
    }

    /// Signs in as `user` through the stand-in provider, the way the desktop
    /// app and the CLI do: the "browser" visits the sign-in address and is
    /// sent back to the listener on this machine.
    pub async fn sign_in(&self, user: &str) -> Result<SessionState, SessionError> {
        let user = user.to_owned();
        self.session
            .sign_in_with_browser(move |address| {
                let visit = format!("{address}&as={user}");
                tokio::spawn(async move {
                    let _ = reqwest::get(visit).await;
                });
                Ok(())
            })
            .await
    }

    /// What the provider would send the browser back to for a sign-in
    /// address, without going there: what Android hands to the app.
    pub async fn redirect_for(address: &str, user: &str) -> String {
        let browser = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("browser");
        let response = browser
            .get(format!("{address}&as={user}"))
            .send()
            .await
            .expect("the provider answers");
        response
            .headers()
            .get("location")
            .expect("a redirect")
            .to_str()
            .expect("text")
            .to_owned()
    }

    /// Gives this device the account key and marks it approved, as another
    /// device will do in step 6. `server` approves it on its side too.
    pub async fn approve_with_key(&self, server: &Server, key: &AccountKey) {
        let mut secrets = DeviceSecrets::read(&*self.secrets)
            .expect("read secrets")
            .expect("signed in");
        server.approve(secrets.device_id).await;
        secrets.account_key = Some(key.clone());
        secrets.approved = true;
        secrets.write(&*self.secrets).expect("write secrets");
        self.session.reload().expect("reload");
    }

    /// The account key this device holds.
    #[must_use]
    pub fn account_key(&self) -> AccountKey {
        DeviceSecrets::read(&*self.secrets)
            .expect("read secrets")
            .expect("signed in")
            .account_key
            .expect("holds the key")
    }

    /// Whether this device's secure storage holds nothing: no token, no
    /// keys.
    #[must_use]
    pub fn secrets_are_empty(&self) -> bool {
        self.secrets.load().expect("read secrets").is_none()
    }

    /// The live notes of the store in use, as `(id, text)` sorted by ID.
    pub async fn notes(&self) -> Vec<(String, String)> {
        let mut notes: Vec<(String, String)> = self
            .session
            .store()
            .list_notes(100_000, 0)
            .await
            .expect("list")
            .into_iter()
            .map(|note| (note.id.to_string(), note.content))
            .collect();
        notes.sort();
        notes
    }
}

/// Waits until `condition` holds, checking often, and panics with `what` if
/// it does not within a few seconds.
pub async fn eventually<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..400 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    panic!("did not happen in time: {what}");
}
