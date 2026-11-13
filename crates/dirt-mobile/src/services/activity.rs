//! What the app asks of `MainActivity`.
//!
//! Android gives some things to the activity and not to the web view the
//! interface runs in: the address the browser comes back to after sign-in,
//! text another app shares, a tap on the home-screen widget, and the screen
//! where a person chooses where to save a file. The activity keeps what it
//! is given in memory, and this module asks for it.
//!
//! - **Sign-in.** [`open_sign_in_page`] opens the page in a browser tab laid
//!   over the app, and [`take_redirect`] gives the address the browser came
//!   back with. That address carries the authorization code, so it is never
//!   written to a file.
//! - **Capture.** [`take_capture`] gives what another app shared, text or
//!   pictures, or the widget's request for a blank note.
//! - **Saving a file.** [`save_copy`] asks the person where to put a copy
//!   of a file the app has, one of a note's files or an export of the
//!   notes, and writes it there; [`take_save_outcome`] says how that went.
//! - **Files for a note.** [`pick_files`] asks the person which files to
//!   attach, and [`take_picked`] gives them once they are copied into the
//!   app. [`open_file`] lends one file to another app, to show.
//!
//! All of it is JNI, as `secure_storage` is, and for the same two reasons
//! classes are loaded through the Activity's class loader and a Java
//! exception is cleared when a call fails. A third applies here: the
//! activity is asked several times a second while a sign-in or a save is
//! under way, so every call runs in a local frame, which frees what the
//! call referred to. A thread that stays attached would otherwise fill its
//! table of references.

#![allow(unsafe_code)]

use jni::objects::{JClass, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};

use crate::incoming::{parse_capture, parse_picked, Capture, Picked};

/// The activity, as the manifest names it.
const ACTIVITY_CLASS: &str = "dev.dioxus.main.MainActivity";

/// Runs `what` with the activity's class.
fn with_activity<T>(
    what: impl FnOnce(&mut JNIEnv<'_>, &JClass<'_>) -> jni::errors::Result<T>,
) -> Result<T, String> {
    let ctx = ndk_context::android_context();

    // SAFETY: `ndk_context::android_context` returns the process-global VM
    // pointer the Android runtime publishes, which is what
    // `JavaVM::from_raw` expects. It is valid for the life of the process.
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|error| format!("invalid JavaVM handle: {error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("attach JVM thread: {error}"))?;

    let result = env.with_local_frame(16, |env| {
        // SAFETY: `ctx.context()` is the host Activity the runtime gave
        // `ndk_context`. It is used as a `JObject` for this call only.
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
        let loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )?
            .l()?;
        let name = env.new_string(ACTIVITY_CLASS)?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )?
            .l()?;
        what(env, &JClass::from(class))
    });
    if result.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    result.map_err(|error| error.to_string())
}

/// Calls a static method of the activity that takes nothing and returns a
/// text or null. What comes back may be private, so it is never logged.
fn take_text(method: &str) -> Option<String> {
    let taken = with_activity(|env, class| {
        let value = env
            .call_static_method(class, method, "()Ljava/lang/String;", &[])?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let text: String = env.get_string(&JString::from(value))?.into();
        Ok(Some(text))
    });
    match taken {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!("MainActivity.{method} could not be called: {error}");
            None
        }
    }
}

/// Calls a static method of the activity that takes one text and returns a
/// text or null.
fn take_text_for(method: &str, text: &str) -> Option<String> {
    let taken = with_activity(|env, class| {
        let given = env.new_string(text)?;
        let value = env
            .call_static_method(
                class,
                method,
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&given)],
            )?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let text: String = env.get_string(&JString::from(value))?.into();
        Ok(Some(text))
    });
    match taken {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!("MainActivity.{method} could not be called: {error}");
            None
        }
    }
}

/// Calls a static method of the activity that takes texts and says whether
/// it could do what was asked.
fn ask(method: &str, texts: &[&str]) -> bool {
    let signature = format!("({})Z", "Ljava/lang/String;".repeat(texts.len()));
    let answered = with_activity(|env, class| {
        let mut held = Vec::with_capacity(texts.len());
        for text in texts {
            held.push(env.new_string(text)?);
        }
        let arguments: Vec<JValue<'_, '_>> = held.iter().map(|text| JValue::Object(text)).collect();
        env.call_static_method(class, method, &signature, &arguments)?
            .z()
    });
    answered.unwrap_or_else(|error| {
        tracing::warn!("MainActivity.{method} could not be called: {error}");
        false
    })
}

/// The redirect address the browser came back with after sign-in, if one
/// is waiting. Taking it removes it.
pub fn take_redirect() -> Option<String> {
    take_text("takeSignInRedirect")
}

/// Opens the sign-in page in a browser tab laid over the app. Returns
/// `false` if the activity could not, in which case the caller opens the
/// browser the ordinary way.
pub fn open_sign_in_page(address: &str) -> bool {
    ask("openSignInPage", &[address])
}

/// What another app shared, or the widget's request, if one is waiting.
/// Taking it removes it.
pub fn take_capture() -> Option<Capture> {
    loop {
        // One that cannot be read is passed over for the next.
        if let Some(capture) = parse_capture(&take_text("takeCapture")?) {
            return Some(capture);
        }
    }
}

/// Asks the person which files to attach, on Android's own screen for
/// that. `request` is a name of the caller's own for this asking, under
/// which [`take_picked`] gives what was chosen: two askings can overlap,
/// and each gets its own files. Returns `false` if the screen could not be
/// asked for, or is already up for another asking.
pub fn pick_files(request: &str) -> bool {
    ask("pickFiles", &[request])
}

/// The files the person chose for `request`, once they are copied into the
/// app, or that they chose none. `None` while they are still choosing.
pub fn take_picked(request: &str) -> Option<Picked> {
    take_text_for("takePicked", request).map(|given| parse_picked(&given))
}

/// Lends the file at `path`, which is inside the app, to whatever other
/// app shows files of this kind. Returns `false` if none took it.
pub fn open_file(path: &str, mime_type: &str) -> bool {
    ask("openFile", &[path, mime_type])
}

/// Asks the person where to save a copy of the file at `path`, which is
/// inside the app, and writes it there. `request` is a name from
/// [`new_request`], under which [`take_save_outcome`] says how it went.
/// Returns `false` if Android's screen for that could not be asked for,
/// or is already up.
pub fn save_copy(request: &str, name: &str, mime_type: &str, path: &str) -> bool {
    ask("saveCopy", &[request, name, mime_type, path])
}

/// A name for one asking of the activity, under which its answer is kept
/// until it is collected. Two askings can overlap, a save still writing
/// when the next is started, and each gets its own answer.
pub fn new_request() -> String {
    static ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "request-{}",
        ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// How saving a file went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved,
    /// The person backed out without choosing a place.
    Cancelled,
    Failed(String),
}

/// How the save asked for as `request` went, once, or `None` while the
/// person is still choosing where or the file is still being written.
pub fn take_save_outcome(request: &str) -> Option<SaveOutcome> {
    let outcome = take_text_for("takeSaveOutcome", request)?;
    Some(match outcome.as_str() {
        "saved" => SaveOutcome::Saved,
        "cancelled" => SaveOutcome::Cancelled,
        other => SaveOutcome::Failed(other.strip_prefix("failed:").unwrap_or(other).to_owned()),
    })
}
