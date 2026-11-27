# What R8 must keep in a release build, which it shrinks and renames
# (`dx` turns minifying on for release). It keeps what Kotlin and the
# manifest use; what only the app's Rust reaches, through JNI and by name,
# it cannot see, and would take away or rename. Dioxus.toml names this file
# ([android] proguard_rules); docs/RELEASING.md.

# The account's keys are kept in AndroidX Security's encrypted preferences,
# which only Rust calls (`services::secure_storage`: MasterKey.Builder,
# EncryptedSharedPreferences.create and their enums).
-keep class androidx.security.crypto.** { *; }
# Tink, under it, names two annotations it does not ship; they are not
# needed to run (R8's own suggestion, missing_rules.txt).
-dontwarn javax.annotation.Nullable
-dontwarn javax.annotation.concurrent.GuardedBy

# The activity's static methods, which Rust calls by name
# (`services::activity`: ACTIVITY_CLASS, and startMicrophone). They are
# marked @Keep in MainActivity.kt too; this does not lean on that.
-keep class dev.dioxus.main.MainActivity {
    public static *;
}

# The native methods Rust provides, found by their classes' and methods'
# names (`Java_dev_dirt_capture_Core_takeIn`, `…_Core_makeWords`,
# `…_Microphone_heard`): their classes and names must not be renamed.
# Android's default rules keep them; this does not lean on those.
-keepclasseswithmembernames class dev.dirt.capture.** {
    native <methods>;
}

# wry's own classes in dev.dioxus.main, which its Rust calls by name, are
# kept by the rules dx generates beside them (proguard-wry.pro).
