// Dirt's native Android code: everything of the app that is not drawn in
// the web view. The capture sheet, the job that makes its thoughts notes
// and syncs once, the Quick Settings tile, the widget and the app shortcut
// (interface design, sections 8.5 and 8.6); the microphone, and the job
// that makes the words of what was said (6.18, 8.9).
//
// The voice folder (the speech model and Silero's detector) is among this
// module's assets, and the engine's libraries among its native libraries,
// both put there by dirt-mobile's `build.rs` and not in git. The model is
// copied out of the APK once, on first use (`VoiceFiles.kt`), since the
// engine opens its files by path. It is left compressed in the APK: stored
// uncompressed it would be 28 MiB larger there, and the copy is made all
// the same (interface design 8.9).
//
// `dx` copies this folder into the Gradle project it generates, as
// `:plugins:dirtnative`, and makes the app depend on it, because
// `src/main.rs` declares it with `#[manganis::ffi("/android/native")]`. The
// plugins carry no versions: the generated project puts the Android Gradle
// plugin and Kotlin on the class path, and `dx` strips versions anyway.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.dirt.capture"
    compileSdk = 34

    defaultConfig {
        minSdk = 24
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}
