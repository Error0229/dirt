package dev.dirt.capture

import android.content.Context

// The app's Rust library, called without the web view: from the jobs, in a
// process where the app's screen may never have started.
//
// The library is the one the app's activity loads (`libmain.so`). Loading
// it runs nothing of the app: the app starts when the activity calls its
// own `create`. The functions are exported by `capture.rs` in dirt-mobile.
object Core {
    init {
        System.loadLibrary("main")
    }

    // Makes a note of every waiting thought, then syncs once if the device
    // is signed in and let in. `context` is any Context: the encrypted
    // preferences that hold the device's secrets are opened with it.
    // `filesDir` is the app's files folder. Returns a line for the log,
    // which says how many and how it went, and nothing of any note.
    @JvmStatic
    external fun takeIn(context: Context, filesDir: String): String

    // Makes a note of every waiting thought and recording, as `takeIn`
    // does, then the words of every recording said on this phone that
    // waits for them (design 6.18, 8.9). It does not sync: `WordsJob` asks
    // `CaptureJob` for that. `voiceDir` is the voice folder with the model
    // in it (`VoiceFiles.forWords`), or empty in a build without voice.
    // Returns a line for the log, which says how many and how it went, and
    // nothing of any note.
    @JvmStatic
    external fun makeWords(context: Context, filesDir: String, voiceDir: String): String
}
