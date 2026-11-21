package dev.dirt.capture

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.util.SizeF
import android.widget.RemoteViews

// The ways in to the capture sheet that are not the app (design 8.1, 8.5,
// 8.6): the Quick Settings tile and the home screen's widget. The app
// shortcut and the note-taking role name the sheet in XML.

private fun sheet(context: Context): Intent =
    Intent(context, CaptureActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

private fun sheetLater(context: Context): PendingIntent =
    PendingIntent.getActivity(
        context,
        0,
        sheet(context),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

// The sheet, listening hands-free: the widget's mic (6.18). Through an
// alias that is not exported, so that only Dirt's own widget can open the
// sheet listening; the sheet itself is exported, for the note-taking role.
private fun listeningLater(context: Context): PendingIntent =
    PendingIntent.getActivity(
        context,
        2,
        Intent().setClassName(context, "dev.dirt.capture.Listen").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

// The Quick Settings tile, "New note". It opens the sheet without asking
// for the phone to be unlocked first (`unlockAndRun` would ask): whether
// the sheet then shows over the lock screen is the phone's choice (8.6).
class CaptureTile : TileService() {
    override fun onStartListening() {
        qsTile?.apply {
            state = Tile.STATE_INACTIVE
            updateTile()
        }
    }

    override fun onClick() {
        if (Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(sheetLater(this))
        } else {
            @Suppress("DEPRECATION", "StartActivityAndCollapseDeprecated")
            startActivityAndCollapse(sheet(this))
        }
    }
}

// The widget: one bar. Its logo opens the app; the bar, the sheet; its mic,
// the sheet listening hands-free, in a build with voice. At one or two cells
// it is only its icons (8.11): a pencil, which opens the sheet as the bar
// does, and the mic. The logo and the icons take the accent the person
// chose, light or dark as the system is, where Android lets a widget's
// colour follow the system (12 and later); before that they are Clay.
class CaptureWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        ids.forEach { id -> manager.updateAppWidget(id, views(context, manager, id)) }
    }

    // Before Android 12 the widget chooses its layout by its width, so it
    // is drawn again when the person resizes it. From 12 the home screen
    // chooses between the two layouts it was given, by itself.
    override fun onAppWidgetOptionsChanged(
        context: Context,
        manager: AppWidgetManager,
        id: Int,
        options: Bundle,
    ) {
        if (Build.VERSION.SDK_INT < 31) manager.updateAppWidget(id, views(context, manager, id))
    }

    companion object {
        // Narrower than this, in dp, the bar is only its icons: one cell or
        // two, where "Write a thought" had no room beside the logo and mic.
        private const val ICONS_BELOW_DP = 180

        private fun views(context: Context, manager: AppWidgetManager, id: Int): RemoteViews =
            if (Build.VERSION.SDK_INT >= 31) {
                RemoteViews(
                    mapOf(
                        SizeF(40f, 40f) to drawn(context, icons = true),
                        SizeF(ICONS_BELOW_DP.toFloat(), 40f) to drawn(context, icons = false),
                    ),
                )
            } else {
                // The width in portrait; 0 before the home screen has said.
                val width = manager.getAppWidgetOptions(id)
                    .getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0)
                drawn(context, icons = width in 1 until ICONS_BELOW_DP)
            }

        private fun drawn(context: Context, icons: Boolean): RemoteViews {
            val layout = if (icons) R.layout.dirt_capture_widget_icons else R.layout.dirt_capture_widget
            val views = RemoteViews(context.packageName, layout)
            views.setOnClickPendingIntent(
                if (icons) R.id.dirt_widget_write else R.id.dirt_widget_bar,
                sheetLater(context),
            )
            if (VoiceFiles.shipped(context)) {
                views.setViewVisibility(R.id.dirt_widget_mic, android.view.View.VISIBLE)
                views.setOnClickPendingIntent(R.id.dirt_widget_mic, listeningLater(context))
            } else {
                views.setViewVisibility(R.id.dirt_widget_mic, android.view.View.GONE)
            }
            if (!icons) {
                context.packageManager.getLaunchIntentForPackage(context.packageName)?.let { app ->
                    views.setOnClickPendingIntent(
                        R.id.dirt_widget_logo,
                        PendingIntent.getActivity(
                            context,
                            1,
                            app,
                            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                        ),
                    )
                }
            }
            if (Build.VERSION.SDK_INT >= 31) {
                val accent = Look.accent(context)
                if (icons) {
                    views.setColorStateList(R.id.dirt_widget_write, "setImageTintList", Look.accentColour(accent))
                } else {
                    views.setColorStateList(R.id.dirt_widget_square, "setImageTintList", Look.accentColour(accent))
                    views.setColorStateList(R.id.dirt_widget_sprout, "setImageTintList", Look.onAccentColour(accent))
                }
                views.setColorStateList(R.id.dirt_widget_mic, "setImageTintList", Look.accentColour(accent))
            }
            return views
        }

        // Draws every Dirt widget again: the accent may have changed. The
        // app calls this as the person leaves it.
        @JvmStatic
        fun refresh(context: Context) {
            val manager = AppWidgetManager.getInstance(context)
            val ids = manager.getAppWidgetIds(ComponentName(context, CaptureWidget::class.java))
            ids.forEach { id -> manager.updateAppWidget(id, views(context, manager, id)) }
        }
    }
}
