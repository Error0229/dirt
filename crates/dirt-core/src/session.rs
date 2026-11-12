//! The session: the one object a client holds.
//!
//! A session knows who is signed in on this device, which note store is in
//! use, and how sync is going. The desktop app, the Android app and the CLI
//! all hold one and render the same four states:
//!
//! - **signed out**: notes are kept in the `local` store on this device;
//! - **signing in**: the browser is open on the sign-in page;
//! - **waiting for approval**: the server knows this device but another
//!   device has not approved it, so it has no key and reads nothing;
//! - **ready**: the device holds the account key, notes are kept in the
//!   account's own store, and they sync.
//!
//! Signing in registers the device with the server. The first device of an
//! account creates the account key. A device whose token expired signs in
//! again and stays the same device.
//!
//! A later device gets the key from an approved one, or from the recovery
//! phrase: see [`devices`].

mod devices;

pub use devices::{Approval, ExchangeStep, Join, MyDevice, ResetOutcome};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dirt_proto::api::{DeviceStatus, ErrorCode, RegisterDeviceRequest};
use tokio::sync::{watch, Notify};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, Transport};
use crate::crypto::{AccountKey, DeviceKeyPair};
use crate::identity::{Attempt, IdentityClient, IdentityError, Loopback};
use crate::secrets::{DeviceSecrets, SecretError, SecretStore};
use crate::store::{store_path, StoreName};
use crate::sync::engine::{SyncEngine, SyncError, SyncReport};
use crate::sync::worker::{SyncStatus, SyncWorker, Timing, Trigger};
use crate::{Error, Store};

/// The file a sign-in attempt is saved in while the browser is open, on
/// platforms that may stop the app meanwhile.
const ATTEMPT_FILE: &str = "sign-in-attempt.json";

/// The file that marks an account's store folder for removal. Sign-out
/// writes it; the folder goes then if nothing holds it open, and otherwise
/// the next time a session opens.
const SIGNED_OUT_MARKER: &str = "signed-out";

/// What a session needs to know about the app it runs in.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// The application's data folder. The stores live under it.
    pub data_folder: PathBuf,
    /// The name other devices will see for this one.
    pub device_name: String,
    /// Whether to run the sync worker. An app that stays open does; the CLI
    /// runs single cycles itself with [`Session::sync_now`].
    pub background_sync: bool,
    /// Whether to save a sign-in attempt to a file before the browser opens.
    /// Android may stop the app while the browser is in front, and the
    /// restarted app must be able to finish the attempt.
    pub persist_attempts: bool,
    /// The sync worker's pauses.
    pub timing: Timing,
}

/// Who is signed in, as an interface shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    SignedOut,
    SigningIn,
    WaitingForApproval { account_id: Uuid, device_id: Uuid },
    Ready { account_id: Uuid, device_id: Uuid },
}

impl SessionState {
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// Why something a session was asked to do failed.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Server(#[from] ApiError),
    #[error(transparent)]
    Secrets(#[from] SecretError),
    #[error(transparent)]
    Store(#[from] Error),
    #[error(transparent)]
    Sync(#[from] SyncError),
    /// The person stopped the sign-in.
    #[error("sign-in was cancelled")]
    Cancelled,
    /// A redirect arrived but no sign-in was under way.
    #[error("no sign-in is in progress")]
    NoAttempt,
    /// The browser could not be opened.
    #[error("could not open the browser: {0}")]
    Browser(String),
    /// The person signed in as someone other than the account this device
    /// holds.
    #[error(
        "this device holds the notes of another account; sign out of it before signing in as someone else"
    )]
    DifferentAccount,
    /// The action needs a device that is signed in and approved.
    #[error("this device is not signed in and approved")]
    NotReady,
    /// The approval exchange with another device failed.
    #[error(transparent)]
    Enrolment(crate::enrolment::EnrolmentError),
    /// The recovery phrase could not be used.
    #[error(transparent)]
    Recovery(crate::recovery::RecoveryError),
    /// The account has no recovery copy for a phrase to open.
    #[error("this account has no recovery phrase")]
    NoRecoveryCopy,
}

/// Why the device was not signed out.
#[derive(Debug, thiserror::Error)]
pub enum SignOutError {
    /// This is the account's only approved device and the account has no
    /// recovery phrase. Signing it out would delete the only copy of the
    /// key that reads the account's notes.
    #[error(
        "this is the only device that can read this account's notes, and the account has no recovery phrase; signing out would make the notes unreadable for good"
    )]
    OnlyApprovedDevice,
    /// Some notes have not reached the server. Signing out would delete
    /// them. The caller may export them and sign out again with
    /// [`Unsynced::Discard`].
    #[error("{notes} note(s) have not been synced and would be lost")]
    Unsynced {
        notes: usize,
        /// Why they could not be synced, if a sync was tried and failed.
        cause: Option<String>,
    },
    /// The server could not be asked which devices the account has.
    #[error("could not check the account's devices: {0}")]
    CannotCheck(ApiError),
    #[error(transparent)]
    Secrets(#[from] SecretError),
    #[error(transparent)]
    Store(#[from] Error),
}

/// What to do at sign-out about notes that never reached the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsynced {
    /// Do not sign out; report how many there are.
    Refuse,
    /// Sign out and delete them.
    Discard,
}

/// A signed-in account's store with what syncs it.
struct Account {
    store: Store,
    engine: Arc<SyncEngine>,
    worker: Option<SyncWorker>,
}

/// The device as it is signed in.
struct SignedIn {
    secrets: DeviceSecrets,
    /// Present once the device is approved and holds the account key.
    account: Option<Account>,
}

impl SignedIn {
    const fn state(&self) -> SessionState {
        let (account_id, device_id) = (self.secrets.account_id, self.secrets.device_id);
        if self.account.is_some() {
            SessionState::Ready {
                account_id,
                device_id,
            }
        } else {
            SessionState::WaitingForApproval {
                account_id,
                device_id,
            }
        }
    }
}

struct Inner {
    config: SessionConfig,
    transport: Arc<dyn Transport>,
    secrets: Arc<dyn SecretStore>,
    identity: IdentityClient,
    local: Store,
    state: watch::Sender<SessionState>,
    status: Arc<watch::Sender<SyncStatus>>,
    signed_in: Mutex<Option<SignedIn>>,
    attempt: Mutex<Option<Attempt>>,
    cancel: Notify,
}

/// See the module documentation. Cloning is cheap; every clone is the same
/// session.
#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Nothing done under these locks can leave the value half-written.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn account_folder(data_folder: &Path, account_id: Uuid) -> Result<PathBuf, Error> {
    let name = StoreName::account(&account_id.to_string())?;
    let database = store_path(data_folder, &name);
    Ok(database
        .parent()
        .map_or_else(|| database.clone(), Path::to_path_buf))
}

/// Removes the store folders of accounts that were signed out of while
/// something still held their files open.
fn sweep_signed_out(data_folder: &Path) {
    let Ok(entries) = std::fs::read_dir(data_folder.join("stores")) else {
        return;
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        if folder.join(SIGNED_OUT_MARKER).exists() {
            if let Err(error) = std::fs::remove_dir_all(&folder) {
                tracing::warn!("could not remove {}: {error}", folder.display());
            }
        }
    }
}

impl Session {
    /// Opens the session for the account on the server that `transport`
    /// reaches, whose secrets are in `secrets`.
    ///
    /// Call this inside a Tokio runtime: a signed-in session with
    /// `background_sync` starts its worker at once.
    pub fn open(
        config: SessionConfig,
        transport: Arc<dyn Transport>,
        secrets: Arc<dyn SecretStore>,
    ) -> Result<Self, SessionError> {
        sweep_signed_out(&config.data_folder);
        let local = Store::open(store_path(&config.data_folder, &StoreName::Local))?;
        let (state, _) = watch::channel(SessionState::SignedOut);
        let (status, _) = watch::channel(SyncStatus::default());
        let session = Self {
            inner: Arc::new(Inner {
                config,
                transport,
                secrets,
                identity: IdentityClient::new()?,
                local,
                state,
                status: Arc::new(status),
                signed_in: Mutex::new(None),
                attempt: Mutex::new(None),
                cancel: Notify::new(),
            }),
        };
        // A sign-in record that cannot be read leaves the device signed out
        // for now. It is not removed: signing in refuses to replace it.
        match DeviceSecrets::read(&*session.inner.secrets) {
            Ok(Some(saved)) => session.activate(saved)?,
            Ok(None) => {}
            Err(error) => tracing::error!("the saved sign-in was not used: {error}"),
        }
        Ok(session)
    }

    /// Who is signed in.
    #[must_use]
    pub fn state(&self) -> SessionState {
        *self.inner.state.borrow()
    }

    /// Who is signed in, as a value that can be waited on for changes.
    #[must_use]
    pub fn watch_state(&self) -> watch::Receiver<SessionState> {
        self.inner.state.subscribe()
    }

    /// How sync is going, as a value that can be waited on for changes.
    #[must_use]
    pub fn watch_sync(&self) -> watch::Receiver<SyncStatus> {
        self.inner.status.subscribe()
    }

    /// How sync is going.
    #[must_use]
    pub fn sync_status(&self) -> SyncStatus {
        self.inner.status.borrow().clone()
    }

    /// The store notes are read from and written to now: the account's when
    /// the device is ready, and the local one otherwise.
    #[must_use]
    pub fn store(&self) -> Store {
        lock(&self.inner.signed_in)
            .as_ref()
            .and_then(|signed_in| signed_in.account.as_ref())
            .map_or_else(|| self.inner.local.clone(), |account| account.store.clone())
    }

    /// The store that belongs to this device whoever is signed in: the
    /// local store. Preferences that describe the device, such as its theme,
    /// are kept here so that they do not change at sign-in.
    #[must_use]
    pub fn device_store(&self) -> Store {
        self.inner.local.clone()
    }

    fn engine(&self) -> Option<Arc<SyncEngine>> {
        lock(&self.inner.signed_in)
            .as_ref()
            .and_then(|signed_in| signed_in.account.as_ref())
            .map(|account| Arc::clone(&account.engine))
    }

    fn saved_secrets(&self) -> Option<DeviceSecrets> {
        lock(&self.inner.signed_in)
            .as_ref()
            .map(|signed_in| signed_in.secrets.clone())
    }

    fn api(&self) -> ApiClient {
        ApiClient::new(Arc::clone(&self.inner.transport))
    }

    /// Puts the device in the state its secrets describe: opens the
    /// account's store and starts sync if it is ready.
    fn activate(&self, secrets: DeviceSecrets) -> Result<(), SessionError> {
        // The previous account's worker and store handles go first.
        lock(&self.inner.signed_in).take();

        let account = match (&secrets.account_key, secrets.approved) {
            (Some(key), true) => Some(self.open_account(&secrets, key)?),
            _ => None,
        };
        let signed_in = SignedIn { secrets, account };
        let state = signed_in.state();
        if let Some(worker) = signed_in
            .account
            .as_ref()
            .and_then(|account| account.worker.as_ref())
        {
            worker.trigger(Trigger::Start);
        }
        *lock(&self.inner.signed_in) = Some(signed_in);
        self.inner.state.send_replace(state);
        Ok(())
    }

    fn open_account(
        &self,
        secrets: &DeviceSecrets,
        key: &AccountKey,
    ) -> Result<Account, SessionError> {
        let config = &self.inner.config;
        let folder = account_folder(&config.data_folder, secrets.account_id)?;
        let marker = folder.join(SIGNED_OUT_MARKER);
        if marker.exists() {
            // Left by a sign-out that could not remove the folder. Start
            // clean if it can be removed now; otherwise keep using it.
            if std::fs::remove_dir_all(&folder).is_err() {
                let _ = std::fs::remove_file(&marker);
            }
        }
        let name = StoreName::account(&secrets.account_id.to_string())?;
        let store = Store::open(store_path(&config.data_folder, &name))?;
        let api = self.api().with_token(secrets.token.clone());
        let engine = Arc::new(SyncEngine::new(
            store.clone(),
            api,
            secrets.account_id,
            key.clone(),
        ));
        self.inner.status.send_replace(SyncStatus::default());
        let worker = config.background_sync.then(|| {
            SyncWorker::spawn(
                Arc::clone(&engine),
                store.clone(),
                Arc::clone(&self.inner.status),
                config.timing,
            )
        });
        Ok(Account {
            store,
            engine,
            worker,
        })
    }

    /// Sets the published state back to what the device's secrets say,
    /// after a sign-in that did not complete.
    fn restore_state(&self) {
        let state = lock(&self.inner.signed_in)
            .as_ref()
            .map_or(SessionState::SignedOut, SignedIn::state);
        self.inner.state.send_replace(state);
    }

    // ---- Signing in ----

    fn attempt_file(&self) -> PathBuf {
        self.inner.config.data_folder.join(ATTEMPT_FILE)
    }

    /// Starts a sign-in and returns the address to open in the system
    /// browser. The provider will send the browser back to `redirect_uri`.
    ///
    /// Android uses this and [`finish_sign_in`](Self::finish_sign_in)
    /// directly. Desktop and the CLI use
    /// [`sign_in_with_browser`](Self::sign_in_with_browser).
    pub async fn begin_sign_in(&self, redirect_uri: &str) -> Result<String, SessionError> {
        // An unreadable record may hold the only copy of an account key.
        // Signing in would replace it, so it has to be dealt with first.
        DeviceSecrets::read(&*self.inner.secrets)?;

        let nonce = self.api().nonce().await?;
        let attempt = Attempt::new(&nonce, redirect_uri)?;
        let address = self.inner.identity.authorization_url(&attempt).await?;
        if self.inner.config.persist_attempts {
            // Saved before the browser opens: the app may not be running
            // when the browser returns.
            std::fs::create_dir_all(&self.inner.config.data_folder).map_err(Error::Io)?;
            let saved = serde_json::to_vec(&attempt).map_err(Error::Serialization)?;
            std::fs::write(self.attempt_file(), saved).map_err(Error::Io)?;
        }
        *lock(&self.inner.attempt) = Some(attempt);
        self.inner.state.send_replace(SessionState::SigningIn);
        Ok(address)
    }

    /// Takes the attempt in progress, from memory or from its file. An
    /// attempt is used once: it is gone before anything is done with it, so
    /// a redirect delivered twice cannot be finished twice.
    fn take_attempt(&self) -> Option<Attempt> {
        let in_memory = lock(&self.inner.attempt).take();
        let file = self.attempt_file();
        let saved = std::fs::read(&file).ok();
        if saved.is_some() {
            let _ = std::fs::remove_file(&file);
        }
        in_memory.or_else(|| saved.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
    }

    /// Finishes the sign-in with the address the provider sent the browser
    /// back to, and returns the new state.
    pub async fn finish_sign_in(&self, redirect: &str) -> Result<SessionState, SessionError> {
        let attempt = self.take_attempt().ok_or(SessionError::NoAttempt)?;
        let result = self.register(&attempt, redirect).await;
        if result.is_err() {
            self.restore_state();
        }
        result
    }

    /// Stops a sign-in that is waiting for the browser.
    pub fn cancel_sign_in(&self) {
        self.inner.cancel.notify_waiters();
        if self.take_attempt().is_some() || self.state() == SessionState::SigningIn {
            self.restore_state();
        }
    }

    /// Signs in through the system browser, which `open_browser` must open
    /// on the address it is given. Returns the new state.
    ///
    /// This waits until the browser comes back or
    /// [`cancel_sign_in`](Self::cancel_sign_in) is called. A sign-in can
    /// fail in the browser without ever returning, so there is no time
    /// limit.
    pub async fn sign_in_with_browser(
        &self,
        open_browser: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<SessionState, SessionError> {
        let loopback = Loopback::bind().await?;
        let address = self.begin_sign_in(loopback.redirect_uri()).await?;
        // Created before the browser opens, so that a cancellation from
        // this point on is not missed.
        let cancelled = self.inner.cancel.notified();
        tokio::pin!(cancelled);
        cancelled.as_mut().enable();

        if let Err(reason) = open_browser(&address) {
            self.take_attempt();
            self.restore_state();
            return Err(SessionError::Browser(reason));
        }
        let Some(attempt) = lock(&self.inner.attempt).clone() else {
            self.restore_state();
            return Err(SessionError::Cancelled);
        };
        let redirect = tokio::select! {
            redirect = loopback.wait_for_redirect(&attempt) => redirect,
            () = &mut cancelled => {
                self.take_attempt();
                self.restore_state();
                return Err(SessionError::Cancelled);
            }
        };
        match redirect {
            Ok(redirect) => self.finish_sign_in(&redirect).await,
            Err(error) => {
                self.take_attempt();
                self.restore_state();
                Err(error.into())
            }
        }
    }

    /// Exchanges the redirect for an ID token and registers the device.
    async fn register(
        &self,
        attempt: &Attempt,
        redirect: &str,
    ) -> Result<SessionState, SessionError> {
        let id_token = self.inner.identity.redeem(attempt, redirect).await?;

        // A device that is already signed in is signing in again, because
        // its token expired or it was removed. It keeps its key pair and
        // shows the server its old token, so that it can stay the device
        // it was.
        let before = self.saved_secrets();
        let device_key = before
            .as_ref()
            .map_or_else(DeviceKeyPair::generate, |saved| saved.device_key.clone());
        let registered = self
            .api()
            .register(&RegisterDeviceRequest {
                id_token,
                nonce: attempt.nonce.clone(),
                name: self.inner.config.device_name.clone(),
                public_key: device_key.public_bytes().to_vec(),
                previous_token: before.as_ref().map(|saved| saved.token.clone()),
            })
            .await?;

        if let Some(saved) = &before {
            if saved.account_id != registered.account_id {
                // The person signed in as someone else. The device that was
                // just registered for that account is of no use: remove it,
                // so that it does not sit there waiting for approval.
                let api = self.api().with_token(registered.token.clone());
                if let Err(error) = api.revoke(registered.device_id).await {
                    tracing::warn!("could not remove the unwanted device: {error}");
                }
                return Err(SessionError::DifferentAccount);
            }
        }

        let account_key = match before.and_then(|saved| saved.account_key) {
            Some(key) => Some(key),
            // The first device of an account creates the account's key.
            None if registered.first_device => Some(AccountKey::generate()),
            None => None,
        };
        let secrets = DeviceSecrets {
            account_id: registered.account_id,
            device_id: registered.device_id,
            token: registered.token,
            device_key,
            account_key,
            approved: registered.status == DeviceStatus::Approved,
        };
        // Saved before anything else: for a first device this record is
        // the only copy of the account key.
        secrets.write(&*self.inner.secrets)?;
        self.activate(secrets)?;
        Ok(self.state())
    }

    /// Asks the server whether a waiting device has been approved, and
    /// returns the state that results.
    pub async fn check_approval(&self) -> Result<SessionState, SessionError> {
        let Some(mut secrets) = self.saved_secrets() else {
            return Ok(self.state());
        };
        if secrets.approved {
            return Ok(self.state());
        }
        let me = self.api().with_token(secrets.token.clone()).me().await?;
        if me.device.status == DeviceStatus::Approved {
            secrets.approved = true;
            secrets.write(&*self.inner.secrets)?;
            self.activate(secrets)?;
        }
        Ok(self.state())
    }

    /// Reads the device's secrets again. The desktop app and the CLI are one
    /// device, so either may sign in or out while the other runs; this is
    /// how the other one notices.
    pub fn reload(&self) -> Result<SessionState, SessionError> {
        if self.state() == SessionState::SigningIn {
            return Ok(self.state());
        }
        let stored = DeviceSecrets::read(&*self.inner.secrets)?;
        let current = self.saved_secrets();
        let same = match (&stored, &current) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.account_id == b.account_id
                    && a.device_id == b.device_id
                    && a.token == b.token
                    && a.is_ready() == b.is_ready()
            }
            _ => false,
        };
        if !same {
            if let Some(secrets) = stored {
                self.activate(secrets)?;
            } else {
                lock(&self.inner.signed_in).take();
                self.inner.status.send_replace(SyncStatus::default());
                self.inner.state.send_replace(SessionState::SignedOut);
            }
        }
        Ok(self.state())
    }

    // ---- Sync ----

    /// Tells the sync worker that something happened. Does nothing when
    /// the device is not ready or has no worker.
    pub fn notify(&self, trigger: Trigger) {
        if let Some(worker) = lock(&self.inner.signed_in)
            .as_ref()
            .and_then(|signed_in| signed_in.account.as_ref())
            .and_then(|account| account.worker.as_ref())
        {
            worker.trigger(trigger);
        }
    }

    /// Runs one sync cycle now and waits for it. This is how the CLI syncs.
    pub async fn sync_now(&self) -> Result<SyncReport, SessionError> {
        let engine = self.engine().ok_or(SessionError::NotReady)?;
        Ok(engine.run_once().await?)
    }

    // ---- Notes written before signing in ----

    /// How many notes written while signed out are waiting in the local
    /// store. They are not part of the account until they are moved.
    pub async fn local_notes_waiting(&self) -> Result<usize, SessionError> {
        if self.state().is_ready() {
            Ok(self.inner.local.note_count().await?)
        } else {
            Ok(0)
        }
    }

    /// Moves the notes written while signed out into the account's store,
    /// from where they sync. Returns how many were moved.
    pub async fn move_local_notes(&self) -> Result<usize, SessionError> {
        let target = self.store();
        if !self.state().is_ready() {
            return Err(SessionError::NotReady);
        }
        let mut moved = 0;
        // A note written to the local store during a pass stays for the
        // next one.
        for _ in 0..5 {
            let pass = self.inner.local.move_notes_into(&target).await?;
            moved += pass;
            if pass == 0 {
                break;
            }
        }
        self.notify(Trigger::Changed);
        Ok(moved)
    }

    /// Removes the notes of accounts this device has signed out of, if any
    /// are still on disk. Sign-out cannot remove them while the interface
    /// holds the store open; the interface calls this once it has let go.
    /// Whatever is still held then goes the next time a session opens.
    pub fn remove_signed_out_stores(&self) {
        sweep_signed_out(&self.inner.config.data_folder);
    }

    // ---- Signing out ----

    /// Signs the device out: syncs, removes the device from the account,
    /// and deletes the account's notes and keys from this device.
    ///
    /// Refused on the account's only approved device unless the account has
    /// a recovery phrase: that device's key is otherwise the only thing that
    /// can read the account's notes. Refused, unless `unsynced` says to
    /// discard them, while notes have not reached the server.
    pub async fn sign_out(&self, unsynced: Unsynced) -> Result<(), SignOutError> {
        let Some(secrets) = self.saved_secrets() else {
            return Ok(());
        };
        let api = self.api().with_token(secrets.token.clone());

        if let Some(engine) = self.engine() {
            match api.me().await {
                // The key survives this device leaving if another approved
                // device holds it, or a recovery phrase can open it.
                Ok(me) => {
                    if me.approvers == 0 && !me.recovery_available {
                        return Err(SignOutError::OnlyApprovedDevice);
                    }
                }
                // A device the server no longer accepts has nothing left to
                // protect there: its key reads nothing new, and another
                // device removed it or it must sign in again anyway.
                Err(error)
                    if matches!(
                        error.code(),
                        Some(ErrorCode::DeviceRevoked | ErrorCode::Unauthenticated)
                    ) => {}
                Err(error) => return Err(SignOutError::CannotCheck(error)),
            }

            let cause = engine.run_once().await.err().map(|error| error.to_string());
            // Neither the engine nor a store handle may outlive this block:
            // the store's files are deleted below.
            drop(engine);
            let waiting = self.store().unsynced_count().await?;
            if waiting > 0 && unsynced == Unsynced::Refuse {
                return Err(SignOutError::Unsynced {
                    notes: waiting,
                    cause,
                });
            }
        }

        // Removing the device makes its token useless at once. If the
        // server cannot be reached the token stays valid until it expires
        // unused, but it leaves this device now either way.
        if let Err(error) = api.revoke(secrets.device_id).await {
            tracing::warn!("the server was not told about the sign-out: {error}");
        }

        self.forget_account(&secrets).map_err(|error| match error {
            SessionError::Secrets(error) => SignOutError::Secrets(error),
            SessionError::Store(error) => SignOutError::Store(error),
            other => SignOutError::Store(Error::Database(other.to_string())),
        })
    }

    /// Removes the account from this device: its keys, and its store with
    /// whatever notes are in it. The device is signed out afterwards.
    fn forget_account(&self, secrets: &DeviceSecrets) -> Result<(), SessionError> {
        // The worker and this session's store handles go before the files.
        let had_account = lock(&self.inner.signed_in)
            .take()
            .is_some_and(|signed_in| signed_in.account.is_some());
        self.inner.secrets.clear()?;
        if had_account {
            let folder = account_folder(&self.inner.config.data_folder, secrets.account_id)?;
            // The mark comes first: if the folder cannot be removed now,
            // because the interface still holds the store, it goes the next
            // time a session opens.
            let _ = std::fs::write(folder.join(SIGNED_OUT_MARKER), b"");
            if let Err(error) = std::fs::remove_dir_all(&folder) {
                tracing::info!("the account's notes will be removed at next start: {error}");
            }
        }
        self.inner.status.send_replace(SyncStatus::default());
        self.inner.state.send_replace(SessionState::SignedOut);
        Ok(())
    }
}

/// A name for this computer that its owner will recognise in a list of
/// devices.
#[must_use]
pub fn this_device_name() -> String {
    ["COMPUTERNAME", "HOSTNAME", "HOST"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|name| name.trim().to_owned())
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "This computer".to_owned())
}

#[cfg(all(feature = "keyring-store", not(target_os = "android")))]
impl Session {
    /// The session of a program on a computer: the desktop app or the CLI.
    /// Both keep their notes under `data_folder` and their secrets in the
    /// operating system's keychain, so they are one device.
    pub fn on_this_computer(
        server_url: &str,
        data_folder: PathBuf,
        background_sync: bool,
    ) -> Result<Self, SessionError> {
        let transport = crate::api::HttpTransport::new(server_url)?;
        let secrets = crate::secrets::KeyringSecrets::for_server(server_url);
        Self::open(
            SessionConfig {
                data_folder,
                device_name: this_device_name(),
                background_sync,
                persist_attempts: false,
                timing: Timing::default(),
            },
            Arc::new(transport),
            Arc::new(secrets),
        )
    }
}
