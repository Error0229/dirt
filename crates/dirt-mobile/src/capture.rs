//! TRIAL 2: capture on Android without the app.
//!
//! The capture sheet is Kotlin with no web view (`android/native`). When
//! the person presses its tick it writes the thought to a file of its own
//! in the app's private storage, named by a UUID v7 it makes on the spot:
//!
//! ```text
//! <files>/dirt/waiting/<note id>.txt
//! ```
//!
//! [`take_in`] turns every waiting file into a note under that ID and
//! removes the file. It is safe to run twice, at once or after a crash: a
//! note that is already there is left alone, and a file is removed only
//! after its note is in the store. Two things call it:
//!
//! - the app, when it starts and each time the person returns to it (the
//!   fallback of design 8.5, and what makes the app's list show the note);
//! - [`Java_dev_dirt_capture_Core_takeIn`], which the sheet's background
//!   job calls through JNI in a process where `MainActivity` may never
//!   have started. It then syncs once.
//!
//! Nothing here uses `ndk_context`: see `EncryptedPrefs::open_with`.

use std::path::{Path, PathBuf};

use dirt_core::models::NoteId;
use dirt_core::Store;

/// The folder of waiting thoughts, under the app's data folder.
pub const WAITING_FOLDER: &str = "waiting";

/// What became of the waiting thoughts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TakenIn {
    /// Notes made now.
    pub made: usize,
    /// Files whose note was there already: taken in before, by this
    /// program or the other one, and not yet removed.
    pub already: usize,
    /// Files left where they are, because the store refused them or they
    /// could not be read.
    pub left: usize,
}

fn waiting_files(data_folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(data_folder.join(WAITING_FOLDER)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        // The sheet writes `<id>.tmp` and renames it, so a `.txt` is whole.
        .filter(|path| path.extension().is_some_and(|kind| kind == "txt"))
        .collect();
    // UUID v7 sorts by time: oldest first.
    files.sort();
    files
}

/// Makes a note of every waiting thought. See the module documentation.
pub async fn take_in(store: &Store, data_folder: &Path) -> TakenIn {
    let mut taken = TakenIn::default();
    for file in waiting_files(data_folder) {
        let id = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<NoteId>().ok())
            // Only a version 7 ID carries the time of capture.
            .filter(|id| id.created_at_ms().is_some());
        let (Some(id), Ok(text)) = (id, std::fs::read_to_string(&file)) else {
            taken.left += 1;
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            let _ = std::fs::remove_file(&file);
            continue;
        }
        match store.create_note_with_id(id, text).await {
            Ok(Some(_)) => {
                taken.made += 1;
                let _ = std::fs::remove_file(&file);
            }
            Ok(None) => {
                taken.already += 1;
                let _ = std::fs::remove_file(&file);
            }
            Err(_) => taken.left += 1,
        }
    }
    taken
}

#[cfg(target_os = "android")]
pub use android::*;

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod android {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Instant;

    use dirt_core::api::HttpTransport;
    use dirt_core::config::ServerAddress;
    use dirt_core::secrets::slot_name;
    use dirt_core::session::{Session, SessionConfig};
    use dirt_core::store::StoreName;
    use dirt_core::sync::worker::Timing;
    use dirt_core::Store;
    use jni::objects::{JClass, JObject, JString};
    use jni::sys::{jboolean, jstring};
    use jni::JNIEnv;

    use super::take_in;
    use crate::bootstrap_config::load_bootstrap_config;
    use crate::services::secure_storage::EncryptedPrefs;

    /// The app's data folder as Kotlin's `Context.getFilesDir()` gives it.
    ///
    /// `config::default_mobile_data_directory` reads the process's name to
    /// find it, which is the package's name only in the app's main process:
    /// in one named `<package>:capture` it would point at a folder that is
    /// not there. Kotlin knows the folder for certain, so it says.
    fn data_folder(files_dir: &str) -> PathBuf {
        Path::new(files_dir).join("dirt")
    }

    fn millis(since: Instant) -> u64 {
        u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Opens the session, takes the waiting thoughts in, and syncs once if
    /// asked and the device is signed in and let in. Returns a JSON report
    /// for the trial's log: counts, the state, and what each part took.
    fn run(
        env: &mut JNIEnv<'_>,
        context: &JObject<'_>,
        files_dir: &str,
        sync: bool,
    ) -> serde_json::Value {
        let started = Instant::now();
        let data_folder = data_folder(files_dir);
        let server =
            match ServerAddress::on_android(load_bootstrap_config().dirt_api_base_url.as_deref()) {
                Ok(server) => server.map(|address| address.url),
                Err(error) => return serde_json::json!({ "error": error.to_string() }),
            };
        let vm = match env.get_java_vm() {
            Ok(vm) => vm,
            Err(error) => return serde_json::json!({ "error": format!("JavaVM: {error}") }),
        };
        // One thread is enough: the store has threads of its own, and one
        // sync is one request after another.
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => return serde_json::json!({ "error": format!("runtime: {error}") }),
        };
        runtime.block_on(async {
            let mut report = serde_json::Map::new();

            let at = Instant::now();
            let mut problem = None;
            let session = server.as_deref().and_then(|url| {
                let opened = EncryptedPrefs::open_with(vm, context, &slot_name(url))
                    .map_err(|error| format!("secrets: {error}"))
                    .and_then(|secrets| {
                        report.insert("secrets_ms".into(), millis(at).into());
                        let transport =
                            HttpTransport::new(url).map_err(|error| error.to_string())?;
                        Session::open(
                            SessionConfig {
                                data_folder: data_folder.clone(),
                                device_name: "Android device".to_owned(),
                                // One cycle, run below, as the CLI does.
                                background_sync: false,
                                persist_attempts: true,
                                timing: Timing::default(),
                            },
                            Arc::new(transport),
                            Arc::new(secrets),
                        )
                        .map_err(|error| format!("session: {error}"))
                    });
                match opened {
                    Ok(session) => Some(session),
                    Err(error) => {
                        problem = Some(error);
                        None
                    }
                }
            });
            // Without a session the thought still becomes a note, in the
            // store a signed-out app uses.
            let store = match &session {
                Some(session) => Ok(session.store()),
                None => Store::open_in(&data_folder, &StoreName::Local),
            };
            let store = match store {
                Ok(store) => store,
                Err(error) => return serde_json::json!({ "error": format!("store: {error}") }),
            };
            report.insert("open_ms".into(), millis(at).into());
            if let Some(problem) = problem {
                report.insert("session_problem".into(), problem.into());
            }

            let at = Instant::now();
            let taken = take_in(&store, &data_folder).await;
            report.insert("take_in_ms".into(), millis(at).into());
            report.insert("made".into(), taken.made.into());
            report.insert("already".into(), taken.already.into());
            report.insert("left".into(), taken.left.into());

            if let Some(session) = &session {
                report.insert("state".into(), format!("{:?}", session.state()).into());
                if sync && session.state().is_ready() {
                    let at = Instant::now();
                    match session.sync_now().await {
                        Ok(cycle) => {
                            report.insert("synced".into(), true.into());
                            report.insert("cycle".into(), format!("{cycle:?}").into());
                        }
                        Err(error) => {
                            report.insert("synced".into(), false.into());
                            report.insert("sync_problem".into(), error.to_string().into());
                        }
                    }
                    report.insert("sync_ms".into(), millis(at).into());
                }
            } else {
                report.insert("state".into(), "NoSession".into());
            }
            report.insert("total_ms".into(), millis(started).into());
            serde_json::Value::Object(report)
        })
    }

    /// `dev.dirt.capture.Core.takeIn(context, filesDir, sync)`.
    ///
    /// Exported by name for JNI. `dx` links the app with
    /// `--export-dynamic`, which is what puts a `#[no_mangle]` function of
    /// this binary crate into the library's symbol table.
    #[no_mangle]
    pub extern "system" fn Java_dev_dirt_capture_Core_takeIn<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        context: JObject<'local>,
        files_dir: JString<'local>,
        sync: jboolean,
    ) -> jstring {
        // A panic must not cross into Java: it would end the process.
        let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let files_dir: String = match env.get_string(&files_dir) {
                Ok(text) => text.into(),
                Err(error) => return serde_json::json!({ "error": format!("filesDir: {error}") }),
            };
            run(&mut env, &context, &files_dir, sync != 0)
        }))
        .unwrap_or_else(|panic| {
            let why = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            serde_json::json!({ "error": format!("panic: {why}") })
        });
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        env.new_string(report.to_string())
            .map_or(std::ptr::null_mut(), JString::into_raw)
    }

    /// `dev.dirt.capture.Core.nothing()`: returns at once. Calling it
    /// loads the library, which is what the trial times.
    #[no_mangle]
    pub extern "system" fn Java_dev_dirt_capture_Core_nothing<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) -> jboolean {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_waiting_thought_becomes_one_note_at_the_time_of_capture() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(folder.path().join("notes.db")).unwrap();
        let waiting = folder.path().join(WAITING_FOLDER);
        std::fs::create_dir_all(&waiting).unwrap();
        // An ID made an hour ago, as the sheet would have made it then.
        let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let since_epoch = hour_ago
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let id = uuid::Uuid::new_v7(uuid::Timestamp::from_unix(
            uuid::NoContext,
            since_epoch.as_secs(),
            since_epoch.subsec_nanos(),
        ));
        std::fs::write(waiting.join(format!("{id}.txt")), "  buy salt #home \n").unwrap();
        std::fs::write(waiting.join(format!("{id}.tmp")), "half written").unwrap();
        std::fs::write(waiting.join("not-an-id.txt"), "x").unwrap();

        let taken = take_in(&store, folder.path()).await;
        assert_eq!((taken.made, taken.already, taken.left), (1, 0, 1));
        let notes = store.list_notes(10, 0).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "buy salt #home");
        assert_eq!(notes[0].id.as_uuid(), id);
        let hour_ago_ms = i64::try_from(since_epoch.as_millis()).unwrap();
        assert_eq!(notes[0].created_at, hour_ago_ms);
        assert_eq!(notes[0].updated_at, hour_ago_ms);
        assert!(!waiting.join(format!("{id}.txt")).exists());

        // The file comes back (a crash before it was removed): no second note.
        std::fs::write(waiting.join(format!("{id}.txt")), "buy salt #home").unwrap();
        let again = take_in(&store, folder.path()).await;
        assert_eq!((again.made, again.already), (0, 1));
        assert_eq!(store.list_notes(10, 0).await.unwrap().len(), 1);
    }
}
