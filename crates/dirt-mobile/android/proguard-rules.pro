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

# The activity's methods that Rust calls (`services::activity`) are marked
# @Keep in MainActivity.kt; Kotlin's native methods, which Rust provides,
# are kept by Android's own rules.
