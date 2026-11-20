package dev.dirt.capture

import android.Manifest
import android.animation.Animator
import android.animation.AnimatorListenerAdapter
import android.animation.ObjectAnimator
import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.graphics.Typeface
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
import android.view.MotionEvent
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
import java.io.File

// The capture sheet (design 8.5): a small screen of its own, drawn over
// whatever was on screen, with the keyboard up. No web view and none of the
// app's Rust: it is ready to type into at once. Its only words are the
// placeholder, and over the lock screen one quiet line (8.6).
//
// ✓ keeps the thought in a file of its own (`Waiting`) and asks for the job
// that makes it a note and syncs once (`CaptureJob`). Back, a tap above the
// sheet or Home closes it, and what was written stays as a draft.
//
// 🎙 listens in the same sheet (design 6.18, 8.9): hold, speak and let go,
// or tap for hands-free, with ✕ and ✓. What is said is written as it is
// heard to a WAV beside the typed thoughts, kept whole when it ends, and
// the job that makes it a note and its words is asked for (`WordsJob`).
// The widget's mic opens the sheet listening hands-free, and Talk, which a
// hardware key can open, listens at once and keeps the thought at a pause.
//
// Over the lock screen it adds and shows nothing: a draft from before is
// not shown, and one written there is added after it. A thought said there
// is kept as one typed there is.
class CaptureActivity : Activity() {
    private lateinit var scrim: View
    private lateinit var sheet: LinearLayout
    private lateinit var field: EditText
    private lateinit var keep: ImageButton
    private var mic: ImageButton? = null
    private lateinit var listening: LinearLayout
    private lateinit var grains: GrainsView
    private lateinit var timer: TextView
    private var talk: Talk? = null
    private var lockLine: TextView? = null
    private var pressX = 0f
    private var pressY = 0f
    private var pressAt = 0L
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
            // A tap above the sheet closes it, keeping what was said.
            setOnClickListener { if (talk != null) end(true) else leave(false) }
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
                    lockLine = this
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
            // An ID of its own, so that Android keeps its text if the sheet
            // is ever made again (a change the sheet does not take itself).
            id = R.id.dirt_capture_field
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
        listening = listeningRow()
        row.addView(listening, LinearLayout.LayoutParams(0, dp(44), 1f))
        if (VoiceFiles.shipped(this)) {
            val button = micButton()
            mic = button
            row.addView(
                button,
                LinearLayout.LayoutParams(dp(40), dp(40)).apply {
                    marginStart = dp(4)
                    bottomMargin = dp(2)
                },
            )
            showMic()
        }
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
        // Made again after a change, the sheet keeps the draft it opened
        // with (the file may hold what was written since), and the field
        // keeps its own text.
        draftBefore = savedInstanceState?.getString(DRAFT_BEFORE) ?: Waiting.draft(this)
        if (savedInstanceState == null && !locked && draftBefore.isNotEmpty()) {
            field.setText(draftBefore)
            field.setSelection(draftBefore.length)
            showKeep(true)
        }
        field.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(text: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(text: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(text: Editable?) = showKeep(!text.isNullOrBlank())
        })
        // Opened to talk (the widget's mic, Talk), the keyboard stays down.
        val asked = if (savedInstanceState == null && mic != null) voiceAsked(intent) else NO_VOICE
        val keyboard = if (asked == NO_VOICE) {
            WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE
        } else {
            WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_HIDDEN
        }
        window.setSoftInputMode(keyboard or WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
        if (asked == NO_VOICE) field.requestFocus()

        root.viewTreeObserver.addOnPreDrawListener(
            object : ViewTreeObserver.OnPreDrawListener {
                override fun onPreDraw(): Boolean {
                    root.viewTreeObserver.removeOnPreDrawListener(this)
                    rise()
                    if (asked != NO_VOICE) listen(handsFree = true, byKey = asked == TALK)
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

    // ✓ springs in when there is text, and goes when there is none. While
    // the sheet listens it keeps what is being said.
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
        if (talk != null) return end(true)
        val text = field.text.toString().trim()
        if (text.isEmpty()) return
        val id = runCatching { Waiting.keep(this, text) }.getOrElse { failure ->
            // Not on the disk: the words stay in the sheet, to keep again or
            // copy.
            // Fixed words: the error's own may name a path, and this can
            // show over the lock screen.
            Log.w(TAG, "the thought could not be kept: $failure")
            say(R.string.dirt_capture_not_kept)
            return
        }
        kept = true
        // Unlocked, the draft was this thought, and goes. Over the lock
        // screen the draft from before was not shown and stays as it was:
        // a pause meanwhile (an assistant over the sheet) may have added
        // this thought to it, which ✓ has now kept.
        Waiting.setDraft(this, if (locked) draftBefore else "")
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

    // Back while it listens keeps what was said, as leaving does.
    @Deprecated("Deprecated in Java")
    override fun onBackPressed() = if (talk != null) end(true) else leave(false)

    // Closed without ✓ (Back, a tap above it, Home, the screen going off):
    // what was written stays as a draft. Listening, what was said is kept:
    // nothing said is lost (6.18).
    override fun onPause() {
        super.onPause()
        if (talk != null) end(true)
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

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putString(DRAFT_BEFORE, draftBefore)
    }

    // Left behind (Home, another app): gone, so that the next opening is a
    // new sheet and not this one brought back. Not when it is only being
    // made again for a change.
    override fun onStop() {
        super.onStop()
        if (!isFinishing && !isChangingConfigurations) finish()
    }

    // What the sheet has to say: a toast, or over the lock screen, where
    // Android shows no toast of an app's, the line above the field, which
    // a screen reader reads.
    private fun say(words: Int) {
        val line = lockLine
        if (line == null) {
            Toast.makeText(this, words, Toast.LENGTH_LONG).show()
            return
        }
        line.setText(words)
        line.announceForAccessibility(getString(words))
    }

    // ---- Voice (6.18, 8.9) ---------------------------------------------

    // One thought being said: the microphone, the file it goes to, and
    // what has been heard.
    private class Talk(
        val listener: Listener,
        val writer: WavWriter,
        val part: File,
        val loudness: Loudness,
        var handsFree: Boolean,
        // Opened by a key, with no hand on the screen: a pause keeps it.
        val byKey: Boolean,
    ) {
        @Volatile var failed = false
    }

    // What the intent asks: to type, to listen hands-free (the widget's
    // mic), or to listen and end on a pause (Talk).
    private fun voiceAsked(intent: Intent?): Int = when {
        intent?.component?.className == TALK_ALIAS -> TALK
        intent?.getBooleanExtra(EXTRA_LISTEN, false) == true -> HANDS_FREE
        else -> NO_VOICE
    }

    // Opened again while it is up (it is a single instance): the widget's
    // mic or Talk listens in the sheet that is there.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val asked = if (mic != null) voiceAsked(intent) else NO_VOICE
        if (asked != NO_VOICE && talk == null) listen(handsFree = true, byKey = asked == TALK)
    }

    private fun micAllowed() =
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    private fun prefs() = getSharedPreferences(PREFS, MODE_PRIVATE)

    // The mic, as it stands: a slash once the microphone was refused, with
    // a tooltip that says what a press does then.
    private fun showMic() {
        val button = mic ?: return
        val refused = !micAllowed() && prefs().getBoolean(ASKED, false)
        button.setImageResource(if (refused) R.drawable.dirt_mic_off else R.drawable.dirt_mic)
        val tip = getString(
            when {
                !refused -> R.string.dirt_mic
                shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO) -> R.string.dirt_mic_refused
                else -> R.string.dirt_mic_refused_for_good
            },
        )
        button.contentDescription = tip
        if (Build.VERSION.SDK_INT >= 26) button.tooltipText = tip
    }

    // The first press asks, in Android's own dialog; the press that asks
    // makes nothing, and the next one listens. Refused for good, Android
    // answers at once without asking, and the sheet says where to allow
    // it. Over the lock screen Android's dialog does not show: the sheet
    // says to unlock.
    private fun askForMic() {
        if (locked) {
            say(R.string.dirt_mic_locked)
            return
        }
        requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), ASK_MIC)
    }

    @Deprecated("Deprecated in Java")
    override fun onRequestPermissionsResult(code: Int, permissions: Array<String>, results: IntArray) {
        @Suppress("DEPRECATION")
        super.onRequestPermissionsResult(code, permissions, results)
        if (code != ASK_MIC) return
        val allowed = results.firstOrNull() == PackageManager.PERMISSION_GRANTED
        if (!allowed) {
            prefs().edit().putBoolean(ASKED, true).apply()
            if (!shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO)) {
                say(R.string.dirt_mic_refused_for_good)
            }
        }
        showMic()
    }

    // The mic: hold to talk and let go to keep; tap for hands-free; slide
    // up to go on hands-free while held; slide left to throw it away. The
    // numbers are the mockup's, as on the desktop (7.11): 280 ms, 60 dp up,
    // 90 dp to the left. While it listens hands-free, a press keeps it.
    private fun micButton(): ImageButton = ImageButton(this).apply {
        imageTintList = ColorStateList.valueOf(getColor(R.color.dirt_text_2))
        background = null
        setOnTouchListener { _, event ->
            val now = talk
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    pressX = event.rawX
                    pressY = event.rawY
                    pressAt = event.eventTime
                    if (now == null) {
                        listen(handsFree = false, byKey = false)
                    } else if (now.handsFree) {
                        end(true)
                    }
                }
                MotionEvent.ACTION_MOVE -> if (now != null && !now.handsFree) {
                    if (pressY - event.rawY > dp(60)) {
                        now.handsFree = true
                        held(false)
                    } else if (pressX - event.rawX > dp(90)) {
                        end(false)
                    }
                }
                MotionEvent.ACTION_UP -> if (now != null && !now.handsFree) {
                    if (event.eventTime - pressAt < HOLD_MS) {
                        now.handsFree = true
                        held(false)
                    } else {
                        end(true)
                    }
                }
                // The press was taken away: what was being said goes.
                MotionEvent.ACTION_CANCEL -> if (now != null && !now.handsFree) end(false)
            }
            true
        }
        // A screen reader's double tap is a tap: hands-free.
        setOnClickListener { if (talk == null) listen(handsFree = true, byKey = false) }
    }

    // The mic, in the accent while it listens, and grown while it is held.
    private fun held(holding: Boolean) {
        val button = mic ?: return
        val colour = if (talk != null) Look.accentColour(Look.accent(this)) else R.color.dirt_text_2
        button.imageTintList = ColorStateList.valueOf(getColor(colour))
        val scale = if (holding) 1.32f else 1f
        button.animate().scaleX(scale).scaleY(scale).setDuration(200)
            .setInterpolator(OvershootInterpolator(1.6f)).start()
    }

    // ✕, the grains and the timer, in the field's place while it listens.
    private fun listeningRow(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        visibility = View.GONE
        addView(
            ImageButton(this@CaptureActivity).apply {
                setImageResource(R.drawable.dirt_x)
                imageTintList = ColorStateList.valueOf(getColor(R.color.dirt_text_2))
                background = null
                contentDescription = getString(R.string.dirt_throw_away)
                if (Build.VERSION.SDK_INT >= 26) tooltipText = getString(R.string.dirt_throw_away)
                setOnClickListener { end(false) }
            },
            LinearLayout.LayoutParams(dp(40), dp(40)),
        )
        grains = GrainsView(this@CaptureActivity).apply {
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        addView(grains, LinearLayout.LayoutParams(0, dp(44), 1f))
        timer = TextView(this@CaptureActivity).apply {
            setTextColor(getColor(R.color.dirt_text_3))
            textSize = 12f
            typeface = Typeface.MONOSPACE
            gravity = Gravity.END or Gravity.CENTER_VERTICAL
            minEms = 2
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        addView(timer, LinearLayout.LayoutParams(WRAP, WRAP).apply { marginEnd = dp(4) })
    }

    // Starts listening: the grains and the timer take the field's place,
    // and the microphone opens. Nothing is written while it listens.
    private fun listen(handsFree: Boolean, byKey: Boolean) {
        if (talk != null || leaving || mic == null) return
        if (!micAllowed()) return askForMic()
        val begun = runCatching {
            val (_, part) = Waiting.newRecording(this)
            part to WavWriter(part)
        }.getOrElse { failure ->
            Log.w(TAG, "the recording could not be begun: $failure")
            say(R.string.dirt_voice_not_kept)
            return
        }
        val (part, writer) = begun
        val loudness = Loudness()
        var now: Talk? = null
        val listener = Listener(this) { piece, count ->
            val failed = runCatching { writer.write(piece, count) }.isFailure
            val level = loudness.take(piece, count)
            val ms = loudness.heardMs
            val speech = loudness.speech
            val quiet = loudness.quietAfterSpeechMs
            runOnUiThread {
                val shown = now ?: return@runOnUiThread
                if (failed) shown.failed = true
                heard(shown, level, ms, speech, quiet)
            }
        }
        val talking = Talk(listener, writer, part, loudness, handsFree, byKey)
        now = talking
        val started = runCatching { listener.start() }.getOrDefault(false)
        if (!started) {
            listener.stop()
            writer.throwAway()
            say(R.string.dirt_mic_failed)
            return
        }
        talk = talking
        getSystemService(InputMethodManager::class.java).hideSoftInputFromWindow(field.windowToken, 0)
        field.visibility = View.GONE
        listening.visibility = View.VISIBLE
        timer.text = ""
        grains.level = 0f
        grains.invalidate()
        showKeep(true)
        held(!handsFree)
        sheet.announceForAccessibility(getString(R.string.dirt_listening))
        Log.i(TAG, "listening locked=$locked byKey=$byKey")
    }

    private fun heard(now: Talk, level: Float, ms: Long, speech: Boolean, quietMs: Long) {
        if (talk !== now) return
        grains.level = level
        val seconds = ms / 1000
        timer.text = if (seconds == 0L) "" else "%d:%02d".format(seconds / 60, seconds % 60)
        when {
            // Ten minutes: it stops, keeping what was said.
            ms >= MOST_MS -> end(true)
            // With no hand on the screen, a pause after speech keeps it,
            // and nothing said at all ends it, as nothing heard.
            now.byKey && speech && quietMs >= PAUSE_MS -> end(true)
            now.byKey && !speech && ms >= NOTHING_SAID_MS -> end(true)
        }
    }

    // Ends listening: kept (✓, letting go, a pause, leaving) or thrown away
    // (✕, a slide to the left). Kept with no speech in it is nothing
    // heard: the sheet shakes once and goes, and nothing is made.
    private fun end(keep: Boolean) {
        val now = talk ?: return
        talk = null
        now.listener.stop()
        held(false)
        if (keep && now.loudness.speech && !now.failed) {
            val whole = runCatching {
                now.writer.finish()
                Waiting.keepRecording(now.part)
            }.getOrDefault(false)
            if (whole) {
                Log.i(TAG, "kept a recording of ${now.writer.durationMs} ms locked=$locked")
                sheet.announceForAccessibility(getString(R.string.dirt_kept))
                runCatching { WordsJob.schedule(applicationContext) }
                    .onFailure { failure -> Log.w(TAG, "the job could not be asked for: $failure") }
                // What was typed stays as a draft, as when the sheet is
                // closed: `onPause` keeps it.
                leave(true)
                return
            }
            now.writer.throwAway()
            say(R.string.dirt_voice_not_kept)
            backToTyping()
            return
        }
        now.writer.throwAway()
        if (keep) {
            sheet.announceForAccessibility(getString(R.string.dirt_nothing_heard))
            val shake = ObjectAnimator.ofFloat(
                sheet,
                "translationX",
                0f,
                -dp(7).toFloat(),
                dp(6).toFloat(),
                -dp(3).toFloat(),
                0f,
            )
            shake.duration = 320
            shake.addListener(object : AnimatorListenerAdapter() {
                override fun onAnimationEnd(animation: Animator) = leave(false)
            })
            shake.start()
        } else {
            sheet.announceForAccessibility(getString(R.string.dirt_thrown))
            backToTyping()
        }
    }

    // Thrown away: the sheet is for typing again, with the keyboard up.
    private fun backToTyping() {
        listening.visibility = View.GONE
        field.visibility = View.VISIBLE
        showKeep(field.text.isNotBlank())
        if (isFinishing || leaving) return
        field.requestFocus()
        getSystemService(InputMethodManager::class.java).showSoftInput(field, 0)
    }

    private companion object {
        const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
        const val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT
        const val DRAFT_BEFORE = "draft_before"

        // Voice: the activity-alias Talk is, the widget's mic's extra, and
        // the numbers of 6.18 and 7.11.
        const val TALK_ALIAS = "dev.dirt.capture.Talk"
        const val EXTRA_LISTEN = "dev.dirt.capture.LISTEN"
        const val NO_VOICE = 0
        const val HANDS_FREE = 1
        const val TALK = 2
        const val ASK_MIC = 4403
        const val PREFS = "dirt_voice"
        const val ASKED = "microphone_asked"
        const val HOLD_MS = 280L
        const val PAUSE_MS = 1600L
        const val NOTHING_SAID_MS = 8000L
        const val MOST_MS = 600_000L

        // The design's curves (5.4).
        val STANDARD = PathInterpolator(0.2f, 0f, 0f, 1f)
        val ENTER = PathInterpolator(0.05f, 0.7f, 0.1f, 1f)
        val EXIT = PathInterpolator(0.3f, 0f, 0.8f, 0.15f)
    }
}
