package dev.dirt.capture

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
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

// The widget: one bar. Its logo opens the app; the bar, the sheet. The
// logo takes the accent the person chose, light or dark as the system is,
// where Android lets a widget's colour follow the system (12 and later);
// before that it is Clay.
class CaptureWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        ids.forEach { id -> manager.updateAppWidget(id, views(context)) }
    }

    companion object {
        private fun views(context: Context): RemoteViews {
            val views = RemoteViews(context.packageName, R.layout.dirt_capture_widget)
            views.setOnClickPendingIntent(R.id.dirt_widget_bar, sheetLater(context))
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
            if (Build.VERSION.SDK_INT >= 31) {
                val accent = Look.accent(context)
                views.setColorStateList(R.id.dirt_widget_square, "setImageTintList", Look.accentColour(accent))
                views.setColorStateList(R.id.dirt_widget_sprout, "setImageTintList", Look.onAccentColour(accent))
            }
            return views
        }

        // Draws every Dirt widget again: the accent may have changed. The
        // app calls this as the person leaves it.
        @JvmStatic
        fun refresh(context: Context) {
            val manager = AppWidgetManager.getInstance(context)
            val ids = manager.getAppWidgetIds(ComponentName(context, CaptureWidget::class.java))
            if (ids.isNotEmpty()) ids.forEach { id -> manager.updateAppWidget(id, views(context)) }
        }
    }
}
