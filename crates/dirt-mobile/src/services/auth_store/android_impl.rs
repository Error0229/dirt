//! Android-side `TokenStore` backed by `EncryptedSharedPreferences`.
//!
//! All JNI ceremony lives in this module so the rest of the mobile
//! shell stays platform-neutral. The `unsafe_code` allow is scoped here
//! and audited: we only construct a `JavaVM` from the raw pointer the
//! Dioxus mobile runtime publishes through `ndk_context`, and only
//! reify the Activity `JObject` from the same source.
//!
//! Threading: each method attaches the current thread (which is fine to
//! call repeatedly — `attach_current_thread` is a no-op when the thread
//! is already attached, and returns an `AttachGuard` that detaches on
//! drop only if it actually did the attach). The `prefs` global ref is
//! shared across threads via the `Sync` impl on `GlobalRef`.

#![allow(unsafe_code)]

use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::JavaVM;

use dirt_core::auth::{StoredToken, TokenStore, TokenStoreResult};

use super::{backend, parse_token_blob, serialize_token_blob};

/// AndroidX EncryptedSharedPreferences-backed token store.
///
/// Constructed once at app startup; cheap to clone (the `JavaVM` and
/// `GlobalRef` are reference-counted handles).
pub struct EncryptedPrefsTokenStore {
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

impl std::fmt::Debug for EncryptedPrefsTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Mirror the desktop `KeyringTokenStore::Debug` shape: surface
        // the slot identifier (useful for diagnosing "which user is
        // this") but never the encrypted blob.
        f.debug_struct("EncryptedPrefsTokenStore")
            .field("pref_key", &self.pref_key)
            .finish_non_exhaustive()
    }
}

impl EncryptedPrefsTokenStore {
    /// Open the EncryptedSharedPreferences file `service` and bind the
    /// store to the `account` key inside it. Building the store touches
    /// the Android KeyStore once (to materialize / unwrap the master
    /// key) so a failure here surfaces immediately rather than at the
    /// first load.
    pub fn open(service: &str, account: &str) -> TokenStoreResult<Self> {
        let ctx = ndk_context::android_context();

        // SAFETY: `ndk_context::android_context` returns the
        // process-global VM pointer the Android runtime publishes via
        // `JNI_OnLoad`; `JavaVM::from_raw` expects exactly that. The
        // pointer is valid for the lifetime of the process.
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
            .map_err(|err| backend(format!("invalid JavaVM handle: {err}")))?;

        let mut env = vm
            .attach_current_thread()
            .map_err(|err| backend(format!("attach JVM thread: {err}")))?;

        // SAFETY: `ctx.context()` returns the host Activity / Application
        // jobject that ndk_context received from the runtime; reifying
        // it as a `JObject` for a single JNIEnv invocation is sound.
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };

        let master_key = build_master_key(&mut env, &activity)?;
        let prefs = create_encrypted_prefs(&mut env, &activity, service, &master_key)?;
        let prefs_ref = env
            .new_global_ref(prefs)
            .map_err(|err| backend(format!("global ref for SharedPreferences: {err}")))?;

        Ok(Self {
            vm,
            prefs: prefs_ref,
            pref_key: account.to_string(),
        })
    }
}

impl TokenStore for EncryptedPrefsTokenStore {
    fn load(&self) -> TokenStoreResult<Option<StoredToken>> {
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
        let json: String = env
            .get_string(&jstr)
            .map_err(|err| backend(format!("decode prefs string: {err}")))?
            .into();
        Ok(Some(parse_token_blob(&json)?))
    }

    fn save(&self, token: &StoredToken) -> TokenStoreResult<()> {
        let json = serialize_token_blob(token)?;
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
            .new_string(&json)
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

    fn clear(&self) -> TokenStoreResult<()> {
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
) -> TokenStoreResult<JObject<'a>> {
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
) -> TokenStoreResult<JObject<'a>> {
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
/// and a failure would never reach the `TokenStore` consumer.
fn commit_editor(env: &mut jni::JNIEnv<'_>, editor: &JObject<'_>) -> TokenStoreResult<()> {
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
