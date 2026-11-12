//! The sign-in hand-off with `MainActivity`.
//!
//! Signing in leaves the app for the browser and comes back through the
//! custom address `dev.dirt.app://callback`. Android gives that address to
//! the activity. This module asks the activity for it, and asks the
//! activity to open the sign-in page.
//!
//! The address carries the authorization code, so the activity holds it in
//! memory and gives it up once. It is not written to a file, as it was
//! while this hand-off was being tried out.
//!
//! All of it is JNI, as `secure_storage` is, and for the same two reasons
//! classes are loaded through the Activity's class loader and a Java
//! exception is cleared when a call fails. A third applies here: the
//! redirect is looked for several times a second while a sign-in is under
//! way, so every call runs in a local frame, which frees what the call
//! referred to. A thread that stays attached would otherwise fill its
//! table of references.

#![allow(unsafe_code)]

use jni::objects::{JClass, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};

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

/// The redirect address the browser came back with, if one is waiting.
/// Taking it removes it.
pub fn take_redirect() -> Option<String> {
    let taken = with_activity(|env, class| {
        let value = env
            .call_static_method(class, "takeSignInRedirect", "()Ljava/lang/String;", &[])?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let text: String = env.get_string(&JString::from(value))?.into();
        Ok(Some(text))
    });
    match taken {
        Ok(redirect) => redirect,
        Err(error) => {
            tracing::warn!("The activity could not be asked for the sign-in redirect: {error}");
            None
        }
    }
}

/// Opens the sign-in page in a browser tab laid over the app. Returns
/// `false` if the activity could not, in which case the caller opens the
/// browser the ordinary way.
pub fn open_sign_in_page(address: &str) -> bool {
    let opened = with_activity(|env, class| {
        let address = env.new_string(address)?;
        env.call_static_method(
            class,
            "openSignInPage",
            "(Ljava/lang/String;)Z",
            &[JValue::Object(&address)],
        )?
        .z()
    });
    opened.unwrap_or_else(|error| {
        tracing::warn!("The activity could not open the sign-in page: {error}");
        false
    })
}
