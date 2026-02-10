package dev.dioxus.main

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.system.Os
import android.widget.RemoteViews

private const val ACTION_QUICK_CAPTURE = "dev.dioxus.main.action.QUICK_CAPTURE"
private const val EXTRA_QUICK_CAPTURE_CONTENT = "dev.dioxus.main.extra.QUICK_CAPTURE_CONTENT"

private const val ENV_QUICK_CAPTURE = "DIRT_QUICK_CAPTURE"
private const val ENV_QUICK_CAPTURE_CONTENT = "DIRT_QUICK_CAPTURE_CONTENT"
private const val ENV_SHARE_TEXT = "DIRT_SHARE_TEXT"

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
    override fun onCreate(savedInstanceState: Bundle?) {
        applyLaunchIntentToEnvironment(intent)
        super.onCreate(savedInstanceState)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        applyLaunchIntentToEnvironment(intent)
    }

    private fun applyLaunchIntentToEnvironment(intent: Intent?) {
        val action = intent?.action.orEmpty()
        val sharedText = if (action == Intent.ACTION_SEND) {
            intent?.getStringExtra(Intent.EXTRA_TEXT)?.trim().orEmpty()
        } else {
            ""
        }
        val quickCaptureText = if (action == ACTION_QUICK_CAPTURE) {
            intent?.getStringExtra(EXTRA_QUICK_CAPTURE_CONTENT)?.trim().orEmpty()
        } else {
            ""
        }
        val quickCaptureEnabled = action == ACTION_QUICK_CAPTURE

        setEnvValue(ENV_SHARE_TEXT, sharedText)
        setEnvValue(ENV_QUICK_CAPTURE_CONTENT, quickCaptureText)
        setEnvValue(ENV_QUICK_CAPTURE, if (quickCaptureEnabled) "true" else "")
    }

    private fun setEnvValue(name: String, value: String) {
        try {
            Os.setenv(name, value, true)
        } catch (_: Exception) {
            // Best effort only.
        }
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
