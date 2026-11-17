// TRIAL 2: Dirt's native Android code, as a Gradle library module.
//
// `dx` copies this folder into the Gradle project it generates, as
// `:plugins:dirtnative`, and makes the app depend on it
// (`#[manganis::ffi("/android/native")]` in `src/main.rs`). The plugins
// carry no versions: the generated root project puts the Android Gradle
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

dependencies {
    // A module can name libraries of its own here, apart from the
    // `gradle_dependencies` of Dioxus.toml. WorkManager is here only to
    // show that it resolves and to measure what it costs; the job itself
    // uses the platform's JobScheduler. Set by the trial's build script.
    // WORKMANAGER implementation("androidx.work:work-runtime:2.9.1")
}
