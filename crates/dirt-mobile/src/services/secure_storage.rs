//! The device's secure storage on Android: `EncryptedSharedPreferences`.
//!
//! The account's secrets (the device token, the device key and the account
//! key) are kept as one value in an app-private preferences file. `AndroidX`
//! Security encrypts it with a master key that is generated inside the
//! Android `KeyStore`, hardware-backed where the device allows, so the file
//! alone is of no use to anyone who copies it.
//!
//! What the value holds is `dirt-core`'s business; this module stores and
//! returns a string. All JNI lives here so that the rest of the app stays
//! platform-neutral. The `unsafe_code` allow is scoped to this module: it
//! only builds a `JavaVM` from the pointer the Dioxus mobile runtime
//! publishes through `ndk_context`, and takes the Activity from the same
//! place.
//!
//! Threading: each method attaches the current thread, which does nothing
//! if it is attached already. The preferences object is a global reference
//! and may be used from any thread.

#![allow(unsafe_code)]

use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::JavaVM;

use dirt_core::secrets::{SecretError, SecretStore};

/// The preferences file the secrets are kept in.
const PREFS_FILE: &str = "dev.dirt.device";

const fn backend(message: String) -> SecretError {
    SecretError::Backend(message)
}

/// One value in the app's encrypted preferences. Opened once at start-up.
pub struct EncryptedPrefs {
    /// `JavaVM` handle obtained from `ndk_context`. `JavaVM` is `Send +
    /// Sync` because each call attaches the current thread on demand.
    vm: JavaVM,
    /// Global reference to the `android.content.SharedPreferences`
    /// instance returned by `EncryptedSharedPreferences.create`. Held
    /// for the lifetime of the store so the JVM doesn't GC it.
    prefs: GlobalRef,
    /// The key (account) we read / write inside the preferences file.
    pref_key: String,
}

impl std::fmt::Debug for EncryptedPrefs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The slot's name helps when diagnosing; what it holds is never
        // printed.
        f.debug_struct("EncryptedPrefs")
            .field("pref_key", &self.pref_key)
            .finish_non_exhaustive()
    }
}

impl EncryptedPrefs {
    /// Opens the slot named `slot` in the encrypted preferences. This
    /// touches the Android `KeyStore` once, to make or unwrap the master
    /// key, so a device where that fails says so here and not at the first
    /// read.
    pub fn open(slot: &str) -> Result<Self, SecretError> {
        let ctx = ndk_context::android_context();

        // SAFETY: `ndk_context::android_context` returns the
        // process-global VM pointer the Android runtime publishes via
        // `JNI_OnLoad`; `JavaVM::from_raw` expects exactly that. The
        // pointer is valid for the lifetime of the process.
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
            .map_err(|err| backend(format!("invalid JavaVM handle: {err}")))?;

        // Inner scope owns the `AttachGuard`. The guard borrows `vm`
        // (so it can detach on drop), so we cannot move `vm` into
        // `Self` while the guard is still alive — explicitly bounding
        // the borrow with this block lets the guard drop before the
        // `Ok(Self { vm, .. })` construction below.
        let prefs_ref = {
            let mut env = vm
                .attach_current_thread()
                .map_err(|err| backend(format!("attach JVM thread: {err}")))?;

            // SAFETY: `ctx.context()` returns the host Activity /
            // Application jobject that ndk_context received from the
            // runtime; reifying it as a `JObject` for a single JNIEnv
            // invocation is sound.
            let activity = unsafe { JObject::from_raw(ctx.context().cast()) };

            let master_key = build_master_key(&mut env, &activity)?;
            let prefs = create_encrypted_prefs(&mut env, &activity, PREFS_FILE, &master_key)?;
            env.new_global_ref(prefs)
                .map_err(|err| backend(format!("global ref for SharedPreferences: {err}")))?
        };

        Ok(Self {
            vm,
            prefs: prefs_ref,
            pref_key: slot.to_string(),
        })
    }
}

impl SecretStore for EncryptedPrefs {
    fn load(&self) -> Result<Option<String>, SecretError> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|err| backend(format!("attach JVM thread: {err}")))?;

        let key = env
            .new_string(&self.pref_key)
            .map_err(|err| backend(format!("alloc pref key string: {err}")))?;
        let null_default = JObject::null();

        let value = env
            .call_method(
                self.prefs.as_obj(),
                "getString",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&key), JValue::Object(&null_default)],
            )
            .map_err(|err| backend(format!("SharedPreferences.getString: {err}")))?
            .l()
            .map_err(|err| backend(format!("getString return value: {err}")))?;

        if value.is_null() {
            return Ok(None);
        }

        let jstr = JString::from(value);
        let stored: String = env
            .get_string(&jstr)
            .map_err(|err| backend(format!("decode prefs string: {err}")))?
            .into();
        Ok(Some(stored))
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|err| backend(format!("attach JVM thread: {err}")))?;

        let editor = env
            .call_method(
                self.prefs.as_obj(),
                "edit",
                "()Landroid/content/SharedPreferences$Editor;",
                &[],
            )
            .map_err(|err| backend(format!("SharedPreferences.edit: {err}")))?
            .l()
            .map_err(|err| backend(format!("edit return value: {err}")))?;

        let key = env
            .new_string(&self.pref_key)
            .map_err(|err| backend(format!("alloc pref key string: {err}")))?;
        let value = env
            .new_string(value)
            .map_err(|err| backend(format!("alloc pref value string: {err}")))?;

        env.call_method(
            &editor,
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&key), JValue::Object(&value)],
        )
        .map_err(|err| backend(format!("Editor.putString: {err}")))?;

        commit_editor(&mut env, &editor)
    }

    fn clear(&self) -> Result<(), SecretError> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|err| backend(format!("attach JVM thread: {err}")))?;

        let editor = env
            .call_method(
                self.prefs.as_obj(),
                "edit",
                "()Landroid/content/SharedPreferences$Editor;",
                &[],
            )
            .map_err(|err| backend(format!("SharedPreferences.edit: {err}")))?
            .l()
            .map_err(|err| backend(format!("edit return value: {err}")))?;

        let key = env
            .new_string(&self.pref_key)
            .map_err(|err| backend(format!("alloc pref key string: {err}")))?;

        env.call_method(
            &editor,
            "remove",
            "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&key)],
        )
        .map_err(|err| backend(format!("Editor.remove: {err}")))?;

        commit_editor(&mut env, &editor)
    }
}

// ---- Helpers (JNI ceremony pulled out so the trait impls stay readable). ----

fn build_master_key<'a>(
    env: &mut jni::JNIEnv<'a>,
    activity: &JObject<'_>,
) -> Result<JObject<'a>, SecretError> {
    // new MasterKey.Builder(context)
    let builder = env
        .new_object(
            "androidx/security/crypto/MasterKey$Builder",
            "(Landroid/content/Context;)V",
            &[JValue::Object(activity)],
        )
        .map_err(|err| backend(format!("new MasterKey.Builder: {err}")))?;

    // KeyScheme.AES256_GCM static field.
    let key_scheme = env
        .get_static_field(
            "androidx/security/crypto/MasterKey$KeyScheme",
            "AES256_GCM",
            "Landroidx/security/crypto/MasterKey$KeyScheme;",
        )
        .map_err(|err| backend(format!("KeyScheme.AES256_GCM lookup: {err}")))?
        .l()
        .map_err(|err| backend(format!("KeyScheme cast: {err}")))?;

    // builder.setKeyScheme(AES256_GCM) → returns the same builder
    let with_scheme = env
        .call_method(
            &builder,
            "setKeyScheme",
            "(Landroidx/security/crypto/MasterKey$KeyScheme;)Landroidx/security/crypto/MasterKey$Builder;",
            &[JValue::Object(&key_scheme)],
        )
        .map_err(|err| backend(format!("Builder.setKeyScheme: {err}")))?
        .l()
        .map_err(|err| backend(format!("setKeyScheme cast: {err}")))?;

    // builder.build() → MasterKey
    let master_key = env
        .call_method(
            &with_scheme,
            "build",
            "()Landroidx/security/crypto/MasterKey;",
            &[],
        )
        .map_err(|err| backend(format!("Builder.build: {err}")))?
        .l()
        .map_err(|err| backend(format!("MasterKey cast: {err}")))?;

    Ok(master_key)
}

fn create_encrypted_prefs<'a>(
    env: &mut jni::JNIEnv<'a>,
    activity: &JObject<'_>,
    file_name: &str,
    master_key: &JObject<'_>,
) -> Result<JObject<'a>, SecretError> {
    let name = env
        .new_string(file_name)
        .map_err(|err| backend(format!("alloc prefs file name: {err}")))?;

    // PrefKeyEncryptionScheme.AES256_SIV
    let pref_key_scheme = env
        .get_static_field(
            "androidx/security/crypto/EncryptedSharedPreferences$PrefKeyEncryptionScheme",
            "AES256_SIV",
            "Landroidx/security/crypto/EncryptedSharedPreferences$PrefKeyEncryptionScheme;",
        )
        .map_err(|err| backend(format!("PrefKeyEncryptionScheme.AES256_SIV: {err}")))?
        .l()
        .map_err(|err| backend(format!("PrefKeyEncryptionScheme cast: {err}")))?;

    // PrefValueEncryptionScheme.AES256_GCM
    let pref_value_scheme = env
        .get_static_field(
            "androidx/security/crypto/EncryptedSharedPreferences$PrefValueEncryptionScheme",
            "AES256_GCM",
            "Landroidx/security/crypto/EncryptedSharedPreferences$PrefValueEncryptionScheme;",
        )
        .map_err(|err| backend(format!("PrefValueEncryptionScheme.AES256_GCM: {err}")))?
        .l()
        .map_err(|err| backend(format!("PrefValueEncryptionScheme cast: {err}")))?;

    // Static call: EncryptedSharedPreferences.create(ctx, name, masterKey, keyScheme, valueScheme)
    let prefs = env
        .call_static_method(
            "androidx/security/crypto/EncryptedSharedPreferences",
            "create",
            "(Landroid/content/Context;Ljava/lang/String;Landroidx/security/crypto/MasterKey;Landroidx/security/crypto/EncryptedSharedPreferences$PrefKeyEncryptionScheme;Landroidx/security/crypto/EncryptedSharedPreferences$PrefValueEncryptionScheme;)Landroid/content/SharedPreferences;",
            &[
                JValue::Object(activity),
                JValue::Object(&name),
                JValue::Object(master_key),
                JValue::Object(&pref_key_scheme),
                JValue::Object(&pref_value_scheme),
            ],
        )
        .map_err(|err| backend(format!("EncryptedSharedPreferences.create: {err}")))?
        .l()
        .map_err(|err| backend(format!("EncryptedSharedPreferences cast: {err}")))?;

    Ok(prefs)
}

/// `SharedPreferences.Editor.commit()` returns a `boolean` — `true` on a
/// successful disk write. We use `commit()` rather than `apply()` so the
/// failure mode surfaces to the caller; `apply()` writes asynchronously
/// and a failure would never reach the caller.
fn commit_editor(env: &mut jni::JNIEnv<'_>, editor: &JObject<'_>) -> Result<(), SecretError> {
    let ok = env
        .call_method(editor, "commit", "()Z", &[])
        .map_err(|err| backend(format!("Editor.commit: {err}")))?
        .z()
        .map_err(|err| backend(format!("commit return value: {err}")))?;
    if ok {
        Ok(())
    } else {
        Err(backend(
            "SharedPreferences.commit returned false (disk write rejected)".into(),
        ))
    }
}
