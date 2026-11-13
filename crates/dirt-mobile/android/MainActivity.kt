package dev.dioxus.main

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.webkit.WebView
import android.widget.RemoteViews
import androidx.annotation.Keep
import androidx.browser.customtabs.CustomTabsIntent
import androidx.core.content.FileProvider
import java.io.File
import java.io.InputStream
import java.io.OutputStream
import java.lang.ref.WeakReference
import org.json.JSONArray
import org.json.JSONObject

private const val ACTION_QUICK_CAPTURE = "dev.dioxus.main.action.QUICK_CAPTURE"
private const val EXTRA_QUICK_CAPTURE_CONTENT = "dev.dioxus.main.extra.QUICK_CAPTURE_CONTENT"

// Marks the answer to the request for a place to save a file.
private const val REQUEST_SAVE_FILE = 4401

// Marks the answer to the request for files to attach.
private const val REQUEST_PICK_FILES = 4402

// The most bytes a file may have and still be attached. It is the server's
// limit for a file (`dirt_proto::api::limits::MAX_FILE_BYTES`), which a
// test of the Rust side holds this line to. A file is copied into the app
// before the Rust side sees it, and a film of two gigabytes is not copied
// to be refused afterwards.
private const val MOST_FILE_BYTES = 10485760L

// Where files are copied to on their way into the app, inside its cache.
private const val INCOMING_FOLDER = "incoming"

// What the file provider in the manifest is called, after the app's name.
private const val FILE_PROVIDER_SUFFIX = ".files"

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

        // What to start notes with, until the Rust side takes them, oldest
        // first: what another app shared, or nothing at all from the
        // home-screen widget, which asks for a blank note. Two may arrive
        // before either is taken. Each is a JSON object with a "text" and
        // a list of "files", as `fileEntry` writes them.
        private val captures = ArrayDeque<String>()

        // How each save went, under the name the Rust side gave the request
        // to save, as the files chosen to attach are kept: a save may still
        // be writing when the person starts another.
        private val saveOutcomes = HashMap<String, String>()

        // The save the screen for choosing a place is up for, if it is up:
        // the request's name, and the path of the file to be written. They
        // are one thing, set together and taken together, so that what one
        // save writes can never be another's.
        private class Saving(val request: String, val copy: String)

        private var saving: Saving? = null

        // The files the person chose to attach, for each request to choose
        // some, under the name the Rust side gave the request: a JSON list
        // once they have all been copied into the app, or "cancelled" if
        // none was chosen. A request has no entry while the person is
        // choosing or the copying goes on.
        //
        // Each request has its own answer because two can overlap: files
        // chosen for one note may still be on their way in when the person
        // goes to another note and chooses again. One shared answer would
        // let either note take the other's files.
        private val picked = HashMap<String, String>()

        // The request the chooser on screen is for, if it is on screen.
        private var pickingFor: String? = null

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
        private fun keepCapture(text: String, files: JSONArray) {
            captures.addLast(JSONObject().put("text", text).put("files", files).toString())
        }

        // The next thing to start a note with, as JSON: a text, which is
        // empty for a blank note, and the files that came with it. Null
        // when nothing is waiting.
        @Keep
        @JvmStatic
        @Synchronized
        fun takeCapture(): String? = captures.removeFirstOrNull()

        @Synchronized
        private fun setPicked(request: String, files: String) {
            picked[request] = files
        }

        // Says the chooser is on screen for `request`, unless it is already
        // on screen for another.
        @Synchronized
        private fun beginPicking(request: String): Boolean {
            if (pickingFor != null) return false
            pickingFor = request
            return true
        }

        // The request the chooser was on screen for, now that it is not.
        @Synchronized
        private fun endPicking(): String? {
            val request = pickingFor
            pickingFor = null
            return request
        }

        // Asks the person which files to attach, with Android's own screen
        // for that. The app needs no permission: it is given the files the
        // person chose. `request` is a name the caller made up, under which
        // it collects what was chosen with takePicked. Returns false when
        // the screen could not be asked for, or is already up for another
        // request.
        @Keep
        @JvmStatic
        fun pickFiles(request: String): Boolean {
            val activity = current.get() ?: return false
            if (!beginPicking(request)) return false
            activity.runOnUiThread {
                runCatching {
                    val choose = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = "*/*"
                        putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                    }
                    activity.startActivityForResult(choose, REQUEST_PICK_FILES)
                }.onFailure {
                    endPicking()
                    setPicked(request, "cancelled")
                }
            }
            return true
        }

        // The files the person chose for `request`, once, as a JSON list of
        // entries each with a "name", a "mime" and either the "path" of a
        // copy inside the app or a "problem"; or "cancelled". Null while
        // the person is still choosing or the files are still being copied.
        @Keep
        @JvmStatic
        @Synchronized
        fun takePicked(request: String): String? = picked.remove(request)

        // Hands a file inside the app to whatever other app shows files of
        // its kind, for reading only. The other app is given this one file
        // and nothing else of this app's. Returns false if no app took it.
        @Keep
        @JvmStatic
        fun openFile(path: String, mimeType: String): Boolean {
            val activity = current.get() ?: return false
            return runCatching {
                val address = FileProvider.getUriForFile(
                    activity,
                    activity.packageName + FILE_PROVIDER_SUFFIX,
                    File(path),
                )
                val view = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(address, mimeType)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                activity.startActivity(Intent.createChooser(view, null))
            }.isSuccess
        }

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

        // Asks the person where to save a copy of the file at `path`, which
        // is inside the app, with Android's own screen for that, and writes
        // it there: one of a note's files, or an export of the notes. The
        // app needs no permission: it is given the one place the person
        // chose. Returns false when the screen could not be asked for, or
        // is already up for another save. How it went is collected with
        // takeSaveOutcome.
        @Keep
        @JvmStatic
        fun saveCopy(request: String, name: String, mimeType: String, path: String): Boolean {
            val activity = current.get() ?: return false
            if (!beginSaving(Saving(request, path))) return false
            activity.runOnUiThread { askForAPlace(activity, request, name, mimeType) }
            return true
        }

        // Puts up Android's screen for choosing where a file goes.
        private fun askForAPlace(
            activity: MainActivity,
            request: String,
            name: String,
            mimeType: String,
        ) {
            runCatching {
                val choose = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = mimeType
                    putExtra(Intent.EXTRA_TITLE, name)
                }
                activity.startActivityForResult(choose, REQUEST_SAVE_FILE)
            }.onFailure { failure ->
                endSaving()
                setSaveOutcome(
                    request,
                    "failed:" + (failure.message ?: failure.javaClass.simpleName),
                )
            }
        }

        @Synchronized
        private fun setSaveOutcome(request: String, outcome: String) {
            saveOutcomes[request] = outcome
        }

        // Says the screen for choosing a place is up for this save, unless
        // it is already up for another.
        @Synchronized
        private fun beginSaving(save: Saving): Boolean {
            if (saving != null) return false
            saving = save
            return true
        }

        // The save that screen was up for, now that it is not: its request
        // and what it writes, taken in one step.
        @Synchronized
        private fun endSaving(): Saving? {
            val save = saving
            saving = null
            return save
        }

        // How the save asked for as `request` went: "saved", "cancelled" or
        // "failed:" and why, once. Null while the person is still choosing
        // or the file is still being written.
        @Keep
        @JvmStatic
        @Synchronized
        fun takeSaveOutcome(request: String): String? = saveOutcomes.remove(request)
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
            // Copies left from files that came in and were never taken.
            File(cacheDir, INCOMING_FOLDER).deleteRecursively()
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
            Intent.ACTION_SEND, Intent.ACTION_SEND_MULTIPLE -> {
                val text = intent.getStringExtra(Intent.EXTRA_TEXT)?.trim().orEmpty()
                // A shared page comes as its address with its title beside
                // it. The title makes the first line of the note.
                val subject = intent.getStringExtra(Intent.EXTRA_SUBJECT)?.trim().orEmpty()
                val note = when {
                    text.isEmpty() -> subject
                    subject.isEmpty() || text.contains(subject) -> text
                    else -> subject + "\n" + text
                }
                val shared = sharedFiles(intent)
                if (shared.isEmpty()) {
                    if (note.isNotEmpty()) keepCapture(note, JSONArray())
                    return
                }
                // The files are read now, while this activity may: the
                // other app lent them for as long as the activity lives.
                // Off the main thread, because a photograph takes a moment.
                Thread {
                    keepCapture(note, copyAllIn(shared))
                    runOnUiThread { tellThePage() }
                }.start()
            }
            ACTION_QUICK_CAPTURE ->
                keepCapture(
                    intent.getStringExtra(EXTRA_QUICK_CAPTURE_CONTENT)?.trim().orEmpty(),
                    JSONArray(),
                )
        }
    }

    // The files another app shared with the intent, if any.
    @Suppress("DEPRECATION")
    private fun sharedFiles(intent: Intent): List<Uri> {
        val many = intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)
        if (many != null) return many.filterNotNull()
        return listOfNotNull(intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM))
    }

    // Copies files that came from outside into the app's cache, and says
    // of each what it is called, what kind it is and where the copy is, or
    // why there is none.
    //
    // Whatever goes wrong with one file is that file's own problem: it
    // gets an entry that says so, and the rest are still copied. This runs
    // on a thread of its own, where a failure nothing caught would end the
    // app, and would leave the Rust side waiting for an answer.
    private fun copyAllIn(files: List<Uri>): JSONArray {
        val entries = JSONArray()
        files.forEach { file ->
            entries.put(
                runCatching { copyIn(file) }.getOrElse { failure ->
                    JSONObject()
                        .put("name", file.lastPathSegment.orEmpty())
                        .put("mime", "")
                        .put("problem", failure.message ?: failure.javaClass.simpleName)
                },
            )
        }
        return entries
    }

    private fun copyIn(file: Uri): JSONObject {
        val entry = JSONObject()
        val name = displayName(file)
        entry.put("name", name)
        // The app that has the file is asked what kind it is, and may fail
        // at that as at anything: the file was taken back, or the app is
        // gone. Then the kind is left out, and read from the name later.
        entry.put("mime", runCatching { contentResolver.getType(file) }.getOrNull() ?: "")
        runCatching {
            // A folder for each file, so that two of one name do not meet.
            val folder = File(File(cacheDir, INCOMING_FOLDER), System.nanoTime().toString())
            folder.mkdirs()
            // Kept under a name of the app's own. What the file is called
            // is carried beside it and never used as a path here.
            val copy = File(folder, "file")
            val whole = contentResolver.openInputStream(file)?.use { from ->
                copy.outputStream().use { to -> copyUpTo(from, to, MOST_FILE_BYTES) }
            } ?: error("the file could not be opened")
            if (whole) {
                entry.put("path", copy.absolutePath)
            } else {
                folder.deleteRecursively()
                entry.put("problem", "too_large")
            }
        }.onFailure { failure ->
            entry.put("problem", failure.message ?: failure.javaClass.simpleName)
        }
        return entry
    }

    // Copies at most `most` bytes. Returns false if there were more, in
    // which case what was copied is of no use.
    private fun copyUpTo(from: InputStream, to: OutputStream, most: Long): Boolean {
        val buffer = ByteArray(64 * 1024)
        var copied = 0L
        while (true) {
            val read = from.read(buffer)
            if (read < 0) return true
            copied += read
            if (copied > most) return false
            to.write(buffer, 0, read)
        }
    }

    // What the person would call the file, as the app that has it says.
    private fun displayName(file: Uri): String {
        val named = runCatching {
            contentResolver.query(file, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { row -> if (row.moveToFirst()) row.getString(0) else null }
        }.getOrNull()
        return named?.takeIf { it.isNotBlank() } ?: file.lastPathSegment.orEmpty()
    }

    // The person chose files to attach, or where to save a file, or backed
    // out of either.
    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        when (requestCode) {
            REQUEST_PICK_FILES -> filesPicked(resultCode, data)
            REQUEST_SAVE_FILE -> placeChosen(resultCode, data)
        }
    }

    private fun filesPicked(resultCode: Int, data: Intent?) {
        // The chooser is off the screen, and another may be asked for
        // while these files are copied in. They stay this request's.
        val request = endPicking() ?: return
        val chosen = mutableListOf<Uri>()
        if (resultCode == RESULT_OK && data != null) {
            // Several files come as a list, one comes by itself.
            val several = data.clipData
            if (several != null) {
                for (index in 0 until several.itemCount) chosen.add(several.getItemAt(index).uri)
            } else {
                data.data?.let { chosen.add(it) }
            }
        }
        if (chosen.isEmpty()) {
            setPicked(request, "cancelled")
            return
        }
        Thread {
            setPicked(request, copyAllIn(chosen).toString())
            runOnUiThread { tellThePage() }
        }.start()
    }

    private fun placeChosen(resultCode: Int, data: Intent?) {
        // The screen is down, and another save may be asked for while this
        // one is written. What is written, and how it went, stay this
        // request's.
        val save = endSaving() ?: return
        val (request, copy) = Pair(save.request, save.copy)
        val place = data?.data
        if (resultCode != RESULT_OK || place == null) {
            setSaveOutcome(request, "cancelled")
            return
        }
        // Off the main thread: a file may be ten megabytes and an export
        // of the notes many times that, and the place may be slow.
        Thread {
            val outcome = runCatching {
                val file = contentResolver.openOutputStream(place)
                    ?: error("the file could not be opened")
                file.use { to ->
                    File(copy).inputStream().use { from -> from.copyTo(to) }
                }
                "saved"
            }.getOrElse { failure ->
                "failed:" + (failure.message ?: failure.javaClass.simpleName)
            }
            setSaveOutcome(request, outcome)
            runOnUiThread { tellThePage() }
        }.start()
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
