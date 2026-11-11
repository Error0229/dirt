package dev.dioxus.main

import android.content.Intent
import android.os.Bundle
import java.io.File

typealias BuildConfig = dev.dirt.trial.signin.BuildConfig

// The scheme registered with Clerk as a redirect target.
private const val REDIRECT_SCHEME = "dev.dirt.app"

class MainActivity : WryActivity() {
    // A cold start: the app was not running when the browser redirected.
    override fun onCreate(savedInstanceState: Bundle?) {
        saveRedirect(intent)
        super.onCreate(savedInstanceState)
    }

    // A warm return: the app was still running behind the browser.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        saveRedirect(intent)
    }

    // Hands the redirect address to Rust by writing it to a file that Rust
    // watches. The rename makes the file appear all at once.
    private fun saveRedirect(intent: Intent?) {
        val address = intent?.data ?: return
        if (intent.action != Intent.ACTION_VIEW || address.scheme != REDIRECT_SCHEME) return
        runCatching {
            val partial = File(filesDir, "redirect.partial")
            partial.writeText(address.toString())
            partial.renameTo(File(filesDir, "redirect.txt"))
        }
    }
}
