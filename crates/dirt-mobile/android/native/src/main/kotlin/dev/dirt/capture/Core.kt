package dev.dirt.capture

import android.content.Context

// TRIAL 2. The app's Rust library, called without the web view.
//
// The library is the one `WryActivity` loads for the app (`libmain.so`).
// Loading it runs nothing of the app: the app starts when the activity
// calls its own `create`. The functions below are exported by
// `crates/dirt-mobile/src/capture.rs`.
object Core {
    init {
        System.loadLibrary("main")
    }

    // Makes a note of every waiting thought, and then syncs once if `sync`
    // and the device is signed in. `context` is any Context: the encrypted
    // preferences are opened with it. `filesDir` is the app's files
    // folder. Returns a JSON report for the log.
    @JvmStatic
    external fun takeIn(context: Context, filesDir: String, sync: Boolean): String?

    // Does nothing. The first call of anything here loads the library.
    @JvmStatic
    external fun nothing(): Boolean
}
