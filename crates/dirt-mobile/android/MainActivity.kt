package dev.dioxus.main

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.webkit.WebView
import android.widget.RemoteViews
import androidx.annotation.Keep
import androidx.browser.customtabs.CustomTabsIntent
import java.lang.ref.WeakReference

private const val ACTION_QUICK_CAPTURE = "dev.dioxus.main.action.QUICK_CAPTURE"
private const val EXTRA_QUICK_CAPTURE_CONTENT = "dev.dioxus.main.extra.QUICK_CAPTURE_CONTENT"

// Marks the answer to the request for a place to save a file.
private const val REQUEST_SAVE_FILE = 4401

// The address scheme the sign-in service sends the browser back to.
private const val SIGN_IN_REDIRECT_SCHEME = "dev.dirt.app"

object BuildConfig {
    val DEBUG: Boolean by lazy {
        runCatching {
            val activityThread = Class.forName("android.app.ActivityThread")
            val app = activityThread.getMethod("currentApplication").invoke(null)
            val packageName = app?.javaClass?.getMethod("getPackageName")?.invoke(app) as? String
            if (packageName.isNullOrBlank()) {
                false
            } else {
                Class.forName("$packageName.BuildConfig").getField("DEBUG").getBoolean(null)
            }
        }.getOrDefault(false)
    }
}

class MainActivity : WryActivity() {
    // What the Rust side calls through JNI (`services::activity`). The
    // methods are found by name at run time, so they are kept through code
    // shrinking. Everything here is static, so it does not depend on which
    // activity object Android has made, and it is held in memory only.
    companion object {
        // The sign-in redirect, until the Rust side takes it. It carries the
        // authorization code, so it is given up once.
        private var signInRedirect: String? = null

        // Texts to start notes with, until the Rust side takes them, oldest
        // first: what another app shared, or nothing at all from the
        // home-screen widget, which asks for a blank note. Two may arrive
        // before either is taken.
        private val captures = ArrayDeque<String>()

        // The text of a file the person is choosing a place for, and how
        // saving it went, until the Rust side takes that.
        private var fileToSave: String? = null
        private var saveOutcome: String? = null

        // The activity on screen, for opening other screens over it.
        private var current = WeakReference<MainActivity>(null)

        @Synchronized
        private fun keepSignInRedirect(address: String) {
            signInRedirect = address
        }

        @Keep
        @JvmStatic
        @Synchronized
        fun takeSignInRedirect(): String? {
            val address = signInRedirect
            signInRedirect = null
            return address
        }

        @Synchronized
        private fun keepCapture(text: String) {
            captures.addLast(text)
        }

        // The next text to start a note with, or an empty text for a blank
        // note, or null when nothing is waiting.
        @Keep
        @JvmStatic
        @Synchronized
        fun takeCapture(): String? = captures.removeFirstOrNull()

        // Opens the sign-in page in a custom tab: a browser tab laid over
        // the app, which closes when the redirect brings the app forward. A
        // browser without custom tabs opens the page as an ordinary tab.
        // Returns false when no browser could be opened this way.
        @Keep
        @JvmStatic
        fun openSignInPage(address: String): Boolean {
            val activity = current.get() ?: return false
            return runCatching {
                CustomTabsIntent.Builder().build().launchUrl(activity, Uri.parse(address))
            }.isSuccess
        }

        @Synchronized
        private fun setFileToSave(text: String?) {
            fileToSave = text
        }

        @Synchronized
        private fun takeFileToSave(): String? {
            val text = fileToSave
            fileToSave = null
            return text
        }

        @Synchronized
        private fun setSaveOutcome(outcome: String?) {
            saveOutcome = outcome
        }

        // Asks the person where to save a file, with Android's own screen
        // for that, and writes `text` there. The app needs no permission:
        // it is given the one place the person chose. Returns false when
        // the screen could not be asked for. How it went is collected with
        // takeSaveOutcome.
        @Keep
        @JvmStatic
        fun saveFile(name: String, mimeType: String, text: String): Boolean {
            val activity = current.get() ?: return false
            setFileToSave(text)
            setSaveOutcome(null)
            activity.runOnUiThread {
                runCatching {
                    val request = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = mimeType
                        putExtra(Intent.EXTRA_TITLE, name)
                    }
                    activity.startActivityForResult(request, REQUEST_SAVE_FILE)
                }.onFailure { failure ->
                    setFileToSave(null)
                    setSaveOutcome("failed:" + (failure.message ?: failure.javaClass.simpleName))
                }
            }
            return true
        }

        // "saved", "cancelled" or "failed:" and why, once, or null while
        // the person is still choosing.
        @Keep
        @JvmStatic
        @Synchronized
        fun takeSaveOutcome(): String? {
            val outcome = saveOutcome
            saveOutcome = null
            return outcome
        }
    }

    // A cold start. If it is the sign-in redirect, the app was stopped while
    // the browser was in front.
    override fun onCreate(savedInstanceState: Bundle?) {
        current = WeakReference(this)
        // Android hands a restored activity the intent it was first started
        // with. What that intent asked for was dealt with before the
        // activity went away: its sign-in code is spent, and what it shared
        // is already a note.
        if (savedInstanceState == null) {
            saveSignInRedirect(intent)
            saveCapture(intent)
        }
        super.onCreate(savedInstanceState)
    }

    // A warm return: the app was still running.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        saveSignInRedirect(intent)
        saveCapture(intent)
        tellThePage()
    }

    // The web view the interface runs in, once the runtime has made it.
    private var page: WebView? = null

    override fun onWebViewCreate(webView: WebView) {
        page = webView
    }

    // Tells the interface that something arrived for it to ask about. The
    // page also asks when it becomes visible again, but an intent can reach
    // an app that never stopped being visible: one sharing the screen with
    // the app that shared, or one under a chooser that does not cover it.
    private fun tellThePage() {
        page?.evaluateJavascript("window.dispatchEvent(new Event('dirt-activity'))", null)
    }

    // Keeps the sign-in redirect for the Rust side, which asks for it.
    private fun saveSignInRedirect(intent: Intent?) {
        val address = intent?.data ?: return
        if (intent.action != Intent.ACTION_VIEW || address.scheme != SIGN_IN_REDIRECT_SCHEME) return
        keepSignInRedirect(address.toString())
    }

    // Keeps what another app shared, or the widget's request for a blank
    // note, for the Rust side.
    private fun saveCapture(intent: Intent?) {
        when (intent?.action) {
            Intent.ACTION_SEND -> {
                val text = intent.getStringExtra(Intent.EXTRA_TEXT)?.trim().orEmpty()
                // A shared page comes as its address with its title beside
                // it. The title makes the first line of the note.
                val subject = intent.getStringExtra(Intent.EXTRA_SUBJECT)?.trim().orEmpty()
                val note = when {
                    text.isEmpty() -> subject
                    subject.isEmpty() || text.contains(subject) -> text
                    else -> subject + "\n" + text
                }
                if (note.isNotEmpty()) keepCapture(note)
            }
            ACTION_QUICK_CAPTURE ->
                keepCapture(intent.getStringExtra(EXTRA_QUICK_CAPTURE_CONTENT)?.trim().orEmpty())
        }
    }

    // The person chose where to save a file, or backed out.
    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_SAVE_FILE) return
        val text = takeFileToSave()
        val place = data?.data
        val outcome = if (resultCode != RESULT_OK || place == null || text == null) {
            "cancelled"
        } else {
            runCatching {
                val file = contentResolver.openOutputStream(place)
                    ?: error("the file could not be opened")
                file.use { it.write(text.toByteArray(Charsets.UTF_8)) }
                "saved"
            }.getOrElse { failure ->
                "failed:" + (failure.message ?: failure.javaClass.simpleName)
            }
        }
        setSaveOutcome(outcome)
    }
}

class QuickCaptureWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(
        context: Context,
        appWidgetManager: AppWidgetManager,
        appWidgetIds: IntArray,
    ) {
        appWidgetIds.forEach { appWidgetId ->
            appWidgetManager.updateAppWidget(appWidgetId, buildViews(context, appWidgetId))
        }
    }

    private fun buildViews(context: Context, appWidgetId: Int): RemoteViews {
        val launchIntent = Intent(context, MainActivity::class.java).apply {
            action = ACTION_QUICK_CAPTURE
            putExtra(EXTRA_QUICK_CAPTURE_CONTENT, "")
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        }

        val pendingIntent = PendingIntent.getActivity(
            context,
            appWidgetId,
            launchIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        return RemoteViews(context.packageName, android.R.layout.simple_list_item_1).apply {
            setTextViewText(android.R.id.text1, "Quick capture")
            setOnClickPendingIntent(android.R.id.text1, pendingIntent)
        }
    }
}
