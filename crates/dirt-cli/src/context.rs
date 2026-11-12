//! What every command works with: the note store in use and, when a server
//! is configured, the session that says who is signed in.
//!
//! The CLI and the desktop app on one computer are one device. They share
//! the data folder and the keychain, so signing in with either signs in
//! both, and both read and write the same notes.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use dirt_core::api::ErrorClass;
use dirt_core::session::{Session, SessionError};
use dirt_core::sync::engine::SyncError;
use dirt_core::Store;

use crate::config_profiles::{normalize_text_option, CliProfilesConfig};
use crate::error::CliError;

/// How long a command that changed a note waits for the sync that follows.
/// The note is already saved; this only bounds how long the prompt takes to
/// come back when the network is slow.
const SYNC_AFTER_WRITE: Duration = Duration::from_secs(10);

pub struct Context {
    /// Where notes are read and written.
    pub store: Store,
    /// Present when a server is configured and no explicit database file
    /// was asked for.
    pub session: Option<Session>,
}

/// The folder the CLI and the desktop app keep their stores in:
/// `DIRT_DATA_DIR` if it is set, which is for trying things out beside a
/// real installation, and otherwise `<data dir>/dirt`.
pub fn data_folder() -> PathBuf {
    if let Some(folder) = env::var_os("DIRT_DATA_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(folder);
    }
    dirs::data_dir()
        .unwrap_or_else(|| panic!("Failed to resolve CLI data directory"))
        .join("dirt")
}

/// The server to sync with: `DIRT_API_BASE_URL`, or else the active
/// profile's address.
pub fn server_url() -> Result<Option<String>, CliError> {
    if let Some(url) = normalize_text_option(env::var("DIRT_API_BASE_URL").ok()) {
        return Ok(Some(url));
    }
    let config = CliProfilesConfig::load().map_err(CliError::Config)?;
    let profile_name = config.resolve_profile_name(None);
    Ok(config
        .profile(&profile_name)
        .and_then(crate::config_profiles::CliProfile::dirt_api_base_url))
}

impl Context {
    /// Opens the store a command should use.
    ///
    /// An explicit database file (`--db-path` or `DIRT_DB_PATH`) is opened
    /// as it is and never syncs. Otherwise the store is the signed-in
    /// account's, or the local one when nobody is signed in.
    pub fn open(cli_db_path: Option<PathBuf>) -> Result<Self, CliError> {
        if let Some(path) = cli_db_path.or_else(|| env::var_os("DIRT_DB_PATH").map(PathBuf::from)) {
            return Ok(Self {
                store: Store::open(path)?,
                session: None,
            });
        }
        let Some(url) = server_url()? else {
            let local =
                dirt_core::store::store_path(&data_folder(), &dirt_core::store::StoreName::Local);
            return Ok(Self {
                store: Store::open(local)?,
                session: None,
            });
        };
        let session = Session::on_this_computer(&url, data_folder(), false)
            .map_err(|error| CliError::Account(error.to_string()))?;
        Ok(Self {
            store: session.store(),
            session: Some(session),
        })
    }

    /// The session, or an explanation of why there is none.
    pub fn require_session(&self) -> Result<&Session, CliError> {
        self.session.as_ref().ok_or_else(|| {
            CliError::Account(
                "no server is configured, or a database file was given explicitly. \
                 Set one with `dirt config init --api-base-url <URL>`"
                    .to_owned(),
            )
        })
    }

    /// Syncs once after a command changed a note, if this device is signed
    /// in. The note is saved whatever happens here, so a failure is a
    /// warning and not an error.
    pub async fn sync_after_write(&self) {
        let Some(session) = &self.session else {
            return;
        };
        if !session.state().is_ready() || env::var_os("DIRT_NO_SYNC").is_some() {
            return;
        }
        match tokio::time::timeout(SYNC_AFTER_WRITE, session.sync_now()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => eprintln!("Saved, but not synced: {}", describe(&error)),
            Err(_) => eprintln!("Saved, but sync is taking too long. Run `dirt sync` later."),
        }
    }
}

/// Words for a failure, by what the person can do about it.
pub fn describe(error: &SessionError) -> String {
    match error {
        SessionError::Sync(sync) => describe_sync(sync),
        SessionError::NotReady => "this device is not signed in. Run `dirt auth login`".to_owned(),
        other => other.to_string(),
    }
}

fn describe_sync(error: &SyncError) -> String {
    match error.class() {
        ErrorClass::TryAgainLater => {
            format!("the server could not be reached ({error}). It will be tried again")
        }
        ErrorClass::StopUntilSignedIn => {
            format!("this device must sign in again ({error}). Run `dirt auth login`")
        }
        ErrorClass::UpdateTheApp => {
            format!("this version of Dirt is too old to sync ({error}). Update it")
        }
        ErrorClass::ProblemWithOneNote => error.to_string(),
    }
}
