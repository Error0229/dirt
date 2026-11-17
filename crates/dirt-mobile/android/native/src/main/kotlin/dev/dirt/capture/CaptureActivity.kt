package dev.dirt.capture

import android.app.Activity
import android.app.KeyguardManager
import android.os.Build
import android.os.Bundle
import android.os.Process
import android.os.SystemClock
import android.text.InputType
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewTreeObserver
import android.view.WindowInsets
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

// TRIAL 2. The capture sheet: a native screen with no web view, drawn over
// whatever was on screen, with the keyboard up. The tick keeps the thought
// in a file and asks for the job that makes it a note and syncs once. The
// Rust library is not loaded here at all, so nothing of it is waited for.
//
// Extras the trial's scripts pass:
//   thread=true        run the work on a plain thread instead of a job
//   expedited=false    an ordinary job instead of an expedited one
//   pause_at, pause_ms see CaptureJobService
//   sync_while_locked=false   over the lock screen, keep and do not sync
open class CaptureActivity : Activity() {
    protected open val ownProcess = false

    private lateinit var field: EditText
    private var locked = false
    private var kept = false
    private var draftBefore = ""
    private val created = SystemClock.uptimeMillis()

    private fun sinceProcessStart() = SystemClock.uptimeMillis() - Process.getStartUptimeMillis()

    private fun dp(value: Int) = TypedValue.applyDimension(
        TypedValue.COMPLEX_UNIT_DIP,
        value.toFloat(),
        resources.displayMetrics,
    ).toInt()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Log.i(
            TAG,
            "sheet onCreate process=${processName()} +${sinceProcessStart()}ms since process start",
        )
        if (Build.VERSION.SDK_INT >= 27) {
            setShowWhenLocked(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED)
        }
        locked = getSystemService(KeyguardManager::class.java).isKeyguardLocked

        val root = FrameLayout(this)
        // A tap above the sheet closes it.
        root.setOnClickListener { finish() }

        val sheet = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundResource(R.drawable.dirt_sheet_bg)
            setPadding(dp(16), dp(12), dp(12), dp(12))
            isClickable = true
        }
        if (locked) {
            sheet.addView(
                TextView(this).apply {
                    setText(R.string.dirt_capture_locked)
                    setTextColor(getColor(R.color.dirt_text_quiet))
                    textSize = 13f
                    setPadding(0, 0, 0, dp(6))
                },
            )
        }
        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        field = EditText(this).apply {
            setHint(R.string.dirt_capture_hint)
            setTextColor(getColor(R.color.dirt_text))
            setHintTextColor(getColor(R.color.dirt_text_quiet))
            background = null
            textSize = 17f
            // Return is a new line.
            inputType = InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_FLAG_MULTI_LINE or
                InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI
            maxLines = 6
            contentDescription = getString(R.string.dirt_capture_hint)
        }
        val tick = Button(this).apply {
            text = "✓"
            contentDescription = getString(R.string.dirt_capture_keep)
            setOnClickListener { keep() }
        }
        row.addView(field, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        row.addView(tick, LinearLayout.LayoutParams(dp(56), dp(48)))
        sheet.addView(row)
        root.addView(
            sheet,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM,
            ),
        )
        setContentView(root)

        // A draft left earlier comes back, but never over the lock screen:
        // there the sheet shows nothing that was written before.
        draftBefore = Waiting.draft(this)
        if (!locked && draftBefore.isNotEmpty()) {
            field.setText(draftBefore)
            field.setSelection(draftBefore.length)
        }

        window.setSoftInputMode(
            WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE or
                WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE,
        )
        field.requestFocus()

        // The trial's clock: the first frame, and the keyboard on screen.
        root.viewTreeObserver.addOnPreDrawListener(
            object : ViewTreeObserver.OnPreDrawListener {
                override fun onPreDraw(): Boolean {
                    root.viewTreeObserver.removeOnPreDrawListener(this)
                    Log.i(
                        TAG,
                        "sheet first frame +${SystemClock.uptimeMillis() - created}ms since " +
                            "onCreate, +${sinceProcessStart()}ms since process start, " +
                            "focused=${field.isFocused} locked=$locked",
                    )
                    return true
                }
            },
        )
        if (Build.VERSION.SDK_INT >= 30) {
            var said = false
            // On the window's own view: with adjustResize the keyboard's
            // inset is used up before it reaches the content.
            window.decorView.setOnApplyWindowInsetsListener { view, insets ->
                if (!said && insets.isVisible(WindowInsets.Type.ime())) {
                    said = true
                    Log.i(
                        TAG,
                        "sheet keyboard visible +${SystemClock.uptimeMillis() - created}ms " +
                            "since onCreate, +${sinceProcessStart()}ms since process start",
                    )
                }
                view.onApplyWindowInsets(insets)
            }
        }
    }

    private fun keep() {
        val text = field.text.toString().trim()
        if (text.isEmpty()) return
        val before = SystemClock.uptimeMillis()
        val id = Waiting.keep(this, text)
        kept = true
        // A draft from before the lock is kept for the unlocked sheet.
        if (!locked) Waiting.setDraft(this, "")
        Log.i(TAG, "sheet kept $id in ${SystemClock.uptimeMillis() - before}ms locked=$locked")

        val pauseAt = intent.getStringExtra("pause_at")
        val pauseMs = intent.getIntExtra("pause_ms", 0)
        if (locked && !intent.getBooleanExtra("sync_while_locked", true)) {
            Log.i(TAG, "sheet left the thought waiting: the phone is locked")
        } else if (intent.getBooleanExtra("thread", false)) {
            // The alternative to a job: a thread that nothing keeps alive
            // once the sheet has closed.
            val context = applicationContext
            Thread {
                if (pauseAt == "before") Thread.sleep(pauseMs.toLong())
                val report = runCatching {
                    Core.takeIn(context, context.filesDir.absolutePath, true)
                }
                Log.i(TAG, "thread done: ${report.getOrElse { failure -> "failed: $failure" }}")
            }.start()
        } else {
            // Whatever goes wrong with asking for the job, the thought is
            // in its file, and the sheet closes as if nothing had.
            runCatching {
                CaptureJobService.schedule(
                    applicationContext,
                    ownProcess,
                    intent.getBooleanExtra("expedited", true),
                    pauseAt,
                    pauseMs,
                )
            }.onFailure { failure -> Log.w(TAG, "the job could not be asked for: $failure") }
        }
        finish()
    }

    // Back, a tap above the sheet, Home: what was written stays as a draft.
    override fun onPause() {
        super.onPause()
        if (kept) return
        val text = field.text.toString()
        if (locked) {
            // Written over the lock screen: added after the earlier draft,
            // which was not shown.
            if (text.isNotBlank()) {
                Waiting.setDraft(this, listOf(draftBefore, text).filter { it.isNotEmpty() }.joinToString("\n"))
            }
        } else {
            Waiting.setDraft(this, text)
        }
    }

    // Left behind (Home, another app): gone, so that the next opening is a
    // new sheet and not this one brought back.
    override fun onStop() {
        super.onStop()
        if (!isFinishing) finish()
    }
}

// The same sheet in the `:capture` process, to compare.
class CaptureActivityOwn : CaptureActivity() {
    override val ownProcess = true
}
