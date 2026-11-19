// Dirt's native Android code: everything of the app that is not drawn in
// the web view. The capture sheet, the job that makes its thoughts notes
// and syncs once, the Quick Settings tile, the widget and the app shortcut
// (interface design, sections 8.5 and 8.6).
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
