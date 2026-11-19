package dev.dirt.capture

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.content.res.ColorStateList
import android.os.Build
import android.os.Bundle
import android.os.Process
import android.os.SystemClock
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.view.WindowInsets
import android.view.WindowManager
import android.view.animation.OvershootInterpolator
import android.view.animation.PathInterpolator
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

// The capture sheet (design 8.5): a small screen of its own, drawn over
// whatever was on screen, with the keyboard up. No web view and none of the
// app's Rust: it is ready to type into at once. Its only words are the
// placeholder, and over the lock screen one quiet line (8.6).
//
// ✓ keeps the thought in a file of its own (`Waiting`) and asks for the job
// that makes it a note and syncs once (`CaptureJob`). Back, a tap above the
// sheet or Home closes it, and what was written stays as a draft.
//
// Over the lock screen it adds and shows nothing: a draft from before is
// not shown, and one written there is added after it.
class CaptureActivity : Activity() {
    private lateinit var scrim: View
    private lateinit var sheet: LinearLayout
    private lateinit var field: EditText
    private lateinit var keep: ImageButton
    private var locked = false
    private var kept = false
    private var leaving = false
    private var keepShown = false
    private var draftBefore = ""
    private val created = SystemClock.uptimeMillis()

    private fun dp(value: Int) = TypedValue.applyDimension(
        TypedValue.COMPLEX_UNIT_DIP,
        value.toFloat(),
        resources.displayMetrics,
    ).toInt()

    // In the theme chosen in the app, not only the system's.
    override fun attachBaseContext(base: Context) {
        super.attachBaseContext(Look.themed(base))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= 27) {
            setShowWhenLocked(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED)
        }
        locked = getSystemService(KeyguardManager::class.java).isKeyguardLocked
        val accent = Look.accent(this)

        val root = FrameLayout(this)
        scrim = View(this).apply {
            setBackgroundColor(getColor(R.color.dirt_scrim))
            alpha = 0f
            // A tap above the sheet closes it.
            setOnClickListener { leave(false) }
        }
        root.addView(scrim, FrameLayout.LayoutParams(MATCH, MATCH))

        sheet = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundResource(R.drawable.dirt_sheet_bg)
            setPadding(dp(20), dp(14), dp(12), dp(12))
            isClickable = true
            elevation = dp(16).toFloat()
        }
        if (locked) {
            sheet.addView(
                TextView(this).apply {
                    setText(R.string.dirt_capture_locked)
                    setTextColor(getColor(R.color.dirt_text_3))
                    textSize = 12f
                    gravity = Gravity.CENTER_VERTICAL
                    compoundDrawablePadding = dp(5)
                    setCompoundDrawablesRelativeWithIntrinsicBounds(R.drawable.dirt_lock, 0, 0, 0)
                    compoundDrawableTintList = ColorStateList.valueOf(getColor(R.color.dirt_text_3))
                },
            )
        }
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.BOTTOM
        }
        field = EditText(this).apply {
            setHint(R.string.dirt_capture_hint)
            setTextColor(getColor(R.color.dirt_text))
            setHintTextColor(getColor(R.color.dirt_text_3))
            background = null
            setPadding(0, dp(8), 0, dp(8))
            textSize = 17f
            if (Build.VERSION.SDK_INT >= 28) lineHeight = dp(24)
            // Return is a new line.
            inputType = InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_FLAG_MULTI_LINE or
                InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            // Over the lock screen the keyboard is asked not to learn from
            // what is typed (8.6).
            imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or
                (if (locked) EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING else 0)
            maxLines = 6
            highlightColor = getColor(Look.accentColour(accent)) and 0x55FFFFFF
        }
        keep = ImageButton(this).apply {
            setImageResource(R.drawable.dirt_check)
            imageTintList = ColorStateList.valueOf(getColor(Look.onAccentColour(accent)))
            setBackgroundResource(R.drawable.dirt_keep_bg)
            backgroundTintList = ColorStateList.valueOf(getColor(Look.accentColour(accent)))
            contentDescription = getString(R.string.dirt_capture_done)
            if (Build.VERSION.SDK_INT >= 26) tooltipText = getString(R.string.dirt_capture_done)
            scaleX = 0f
            scaleY = 0f
            alpha = 0f
            isEnabled = false
            setOnClickListener { keepIt() }
        }
        row.addView(field, LinearLayout.LayoutParams(0, WRAP, 1f))
        row.addView(
            keep,
            LinearLayout.LayoutParams(dp(40), dp(40)).apply {
                marginStart = dp(6)
                bottomMargin = dp(2)
            },
        )
        sheet.addView(row)
        root.addView(sheet, FrameLayout.LayoutParams(MATCH, WRAP, Gravity.BOTTOM))
        setContentView(root)
        fitAboveTheKeyboard(root)

        // A draft left earlier comes back, but never over the lock screen.
        draftBefore = Waiting.draft(this)
        if (!locked && draftBefore.isNotEmpty()) {
            field.setText(draftBefore)
            field.setSelection(draftBefore.length)
            showKeep(true)
        }
        field.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(text: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(text: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(text: Editable?) = showKeep(!text.isNullOrBlank())
        })
        window.setSoftInputMode(
            WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE or
                WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE,
        )
        field.requestFocus()

        root.viewTreeObserver.addOnPreDrawListener(
            object : ViewTreeObserver.OnPreDrawListener {
                override fun onPreDraw(): Boolean {
                    root.viewTreeObserver.removeOnPreDrawListener(this)
                    rise()
                    Log.i(
                        TAG,
                        "sheet drawn ${SystemClock.uptimeMillis() - created} ms after it was made, " +
                            "${SystemClock.uptimeMillis() - Process.getStartUptimeMillis()} ms " +
                            "after its process started; locked=$locked",
                    )
                    return true
                }
            },
        )
    }

    // The sheet sits on the keyboard, or on the bottom of the screen, clear
    // of the navigation bar. From Android 11 the window reaches under the
    // bars and the keyboard, and the sheet keeps clear of them itself.
    private fun fitAboveTheKeyboard(root: FrameLayout) {
        if (Build.VERSION.SDK_INT < 30) return
        window.setDecorFitsSystemWindows(false)
        root.setOnApplyWindowInsetsListener { _, insets ->
            val keyboard = insets.getInsets(WindowInsets.Type.ime()).bottom
            val bars = insets.getInsets(WindowInsets.Type.systemBars()).bottom
            root.setPadding(0, 0, 0, keyboard)
            sheet.setPadding(dp(20), dp(14), dp(12), dp(12) + if (keyboard > 0) 0 else bars)
            insets
        }
    }

    // Opening: the screen behind dims, and the sheet rises from the bottom
    // with the keyboard (400 ms, the enter curve; the dimming 280 ms,
    // standard). With "Remove animations" Android runs these at once.
    private fun rise() {
        sheet.translationY = (sheet.height + dp(24)).toFloat()
        sheet.animate().translationY(0f).setDuration(400).setInterpolator(ENTER).start()
        scrim.animate().alpha(1f).setDuration(280).setInterpolator(STANDARD).start()
    }

    // ✓ springs in when there is text, and goes when there is none.
    private fun showKeep(shown: Boolean) {
        if (shown == keepShown) return
        keepShown = shown
        keep.isEnabled = shown
        keep.animate().cancel()
        if (shown) {
            keep.animate().scaleX(1f).scaleY(1f).alpha(1f).setDuration(280)
                .setInterpolator(OvershootInterpolator(2.2f)).start()
        } else {
            keep.animate().scaleX(0f).scaleY(0f).alpha(0f).setDuration(120)
                .setInterpolator(EXIT).start()
        }
    }

    // ✓: the thought is kept on the phone at once, and the job is asked for.
    // Nothing says "Saved".
    private fun keepIt() {
        if (leaving) return
        val text = field.text.toString().trim()
        if (text.isEmpty()) return
        val id = runCatching { Waiting.keep(this, text) }.getOrElse { failure ->
            // Not on the disk: the words stay in the sheet, to keep again or
            // copy.
            Log.w(TAG, "the thought could not be kept: $failure")
            Toast.makeText(this, failure.message ?: "The thought could not be kept", Toast.LENGTH_LONG).show()
            return
        }
        kept = true
        // A draft from before the lock was not shown, and stays for the
        // unlocked sheet.
        if (!locked) Waiting.setDraft(this, "")
        Log.i(TAG, "kept $id locked=$locked")
        // Whatever goes wrong asking for the job, the thought is in its file
        // and the app takes it in when it next starts or comes back.
        runCatching { CaptureJob.schedule(applicationContext) }
            .onFailure { failure -> Log.w(TAG, "the job could not be asked for: $failure") }
        leave(true)
    }

    // Leaving: kept, the sheet lifts 24 dp and fades (240 ms, the exit
    // curve); closed, it sinks back (300 ms). The dimming goes with it.
    private fun leave(kept: Boolean) {
        if (leaving) return
        leaving = true
        getSystemService(InputMethodManager::class.java)
            .hideSoftInputFromWindow(field.windowToken, 0)
        val done = Runnable {
            finish()
            @Suppress("DEPRECATION")
            overridePendingTransition(0, 0)
        }
        if (kept) {
            sheet.animate().translationY(-dp(24).toFloat()).alpha(0f).setDuration(240)
                .setInterpolator(EXIT).withEndAction(done).start()
            scrim.animate().alpha(0f).setDuration(240).setInterpolator(STANDARD).start()
        } else {
            sheet.animate().translationY((sheet.height + dp(24)).toFloat()).setDuration(300)
                .setInterpolator(EXIT).withEndAction(done).start()
            scrim.animate().alpha(0f).setDuration(300).setInterpolator(STANDARD).start()
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() = leave(false)

    // Closed without ✓ (Back, a tap above it, Home, the screen going off):
    // what was written stays as a draft.
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

    private companion object {
        const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
        const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT

        // The design's curves (5.4).
        val STANDARD = PathInterpolator(0.2f, 0f, 0f, 1f)
        val ENTER = PathInterpolator(0.05f, 0.7f, 0.1f, 1f)
        val EXIT = PathInterpolator(0.3f, 0f, 0.8f, 0.15f)
    }
}
