package dev.dirt.capture

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.util.Log
import android.widget.RemoteViews

// TRIAL 2. The ways in to the capture sheet that are not the app.

private fun openSheet(context: Context): Intent =
    Intent(context, CaptureActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

private fun openSheetLater(context: Context): PendingIntent =
    PendingIntent.getActivity(
        context,
        0,
        openSheet(context),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

// The Quick Settings tile, "New note". It opens the sheet without asking
// for the phone to be unlocked first (`unlockAndRun` would ask): whether
// the sheet then shows over the lock screen is the phone's choice.
class CaptureTileService : TileService() {
    override fun onStartListening() {
        qsTile?.apply {
            state = Tile.STATE_INACTIVE
            updateTile()
        }
    }

    override fun onClick() {
        Log.i(TAG, "tile clicked locked=$isLocked secure=$isSecure")
        if (Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(openSheetLater(this))
        } else {
            @Suppress("DEPRECATION", "StartActivityAndCollapseDeprecated")
            startActivityAndCollapse(openSheet(this))
        }
    }
}

// The bar widget, drawn from a layout resource of this module.
class CaptureWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        ids.forEach { id ->
            val views = RemoteViews(context.packageName, R.layout.dirt_capture_widget)
            views.setOnClickPendingIntent(R.id.dirt_widget_bar, openSheetLater(context))
            manager.updateAppWidget(id, views)
        }
    }
}

// Says in the log whether Android told the app that the phone was
// unlocked. Declared in the manifest, so it would be started for it.
class UnlockedReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Log.i(TAG, "USER_PRESENT received in ${processName()} waiting=${Waiting.count(context)}")
    }
}
