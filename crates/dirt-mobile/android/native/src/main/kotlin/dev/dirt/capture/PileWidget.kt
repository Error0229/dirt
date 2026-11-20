package dev.dirt.capture

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RadialGradient
import android.graphics.Shader
import android.os.Bundle
import android.util.Log
import android.view.View
import android.widget.RemoteViews
import java.io.File
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

// The pile widget (design 8.5, 6.17): the person's pile on the home screen,
// two cells by two, with one line under it, "1,254 thoughts". Tapping it
// opens Dirt.
//
// A widget cannot run the app's Rust or its web view, nor draw SVG. The app
// writes the heap to a small file whenever it is worked out again
// (`pile_widget` in dirt-mobile): where each grain lies and in which shade,
// and the line. The widget draws those dots into a picture. It is drawn
// again as the person leaves the app, and when the capture sheet's work has
// taken a thought in, so that a thought kept from the sheet shows as one
// more grain: a home-screen widget cannot play the grain falling. The file
// holds no note's words, so the widget is as private as the bar.
class PileWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        ids.forEach { id -> draw(context, manager, id) }
    }

    override fun onAppWidgetOptionsChanged(
        context: Context,
        manager: AppWidgetManager,
        id: Int,
        options: Bundle,
    ) {
        draw(context, manager, id)
    }

    // What the app wrote: the line, and the heap if there are grains.
    private class Pile(
        val line: String,
        val box: IntArray?,
        val shadow: IntArray?,
        val grains: List<Pair<Int, FloatArray>>,
    )

    companion object {
        // The most pixels the picture has: a widget's picture is handed to
        // the home screen whole, through a channel of about a megabyte. A
        // heap larger than this on the screen is drawn at less than the
        // screen's density and scaled up.
        private const val MOST_PIXELS = 180_000

        private fun file(context: Context) = File(Waiting.dataFolder(context), "pile")

        private fun read(context: Context): Pile? {
            val lines = runCatching { file(context).readLines() }.getOrNull() ?: return null
            if (lines.size < 2 || lines[0] != "dirt-pile 1") return null
            if (lines.size < 4) return Pile(lines[1], null, null, emptyList())
            val numbers = { line: String -> line.trim().split(' ').map { it.toInt() }.toIntArray() }
            return runCatching {
                val grains = lines.drop(4).filter { it.isNotBlank() }.map { line ->
                    val words = line.trim().split(' ')
                    val colour = Color.parseColor(words[0])
                    val dots = FloatArray(words.size - 1) { index -> words[index + 1].toFloat() }
                    Pair(colour, dots)
                }
                Pile(lines[1], numbers(lines[2]), numbers(lines[3]), grains)
            }.getOrNull()
        }

        // The heap drawn as it is under the list: each grain a dot of its
        // shade, a soft shadow under the heap. As large as on the list, in
        // dp, and smaller if the widget is narrower: then the grains get
        // smaller, never fewer.
        private fun picture(context: Context, pile: Pile, widthDp: Int, heightDp: Int): Bitmap? {
            val box = pile.box ?: return null
            val shadow = pile.shadow ?: return null
            if (pile.grains.isEmpty()) return null
            // Tenths of a CSS pixel, which is a dp here.
            val (left, top, width, height) = box.take(4)
            val grain = box[4].toFloat()
            val density = context.resources.displayMetrics.density
            // The picture is the widget's room, so that the home screen
            // never enlarges it: the heap lies in its middle, as large as
            // under the list, or smaller to fit.
            val fit = min(1f, min(widthDp * 10f / width, heightDp * 10f / height))
            var pixelsWide = widthDp * density
            var pixelsHigh = heightDp * density
            var scale = density / 10f * fit
            val area = pixelsWide * pixelsHigh
            if (area > MOST_PIXELS) {
                val less = kotlin.math.sqrt(MOST_PIXELS / area)
                scale *= less
                pixelsWide *= less
                pixelsHigh *= less
            }
            val bitmap = Bitmap.createBitmap(
                max(1, pixelsWide.roundToInt()),
                max(1, pixelsHigh.roundToInt()),
                Bitmap.Config.ARGB_8888,
            )
            val canvas = Canvas(bitmap)
            canvas.translate((pixelsWide - width * scale) / 2f, (pixelsHigh - height * scale) / 2f)
            canvas.scale(scale, scale)
            canvas.translate(-left.toFloat(), -top.toFloat())
            // The shadow: an ellipse, its centre darkest.
            val (cx, cy, rx, ry) = shadow.take(4)
            canvas.save()
            canvas.scale(1f, ry.toFloat() / max(1, rx))
            val shade = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                this.shader = RadialGradient(
                    cx.toFloat(),
                    cy * max(1, rx).toFloat() / max(1, ry),
                    max(1, rx).toFloat(),
                    Color.argb(71, 0, 0, 0),
                    Color.TRANSPARENT,
                    Shader.TileMode.CLAMP,
                )
            }
            canvas.drawCircle(cx.toFloat(), cy * max(1, rx).toFloat() / max(1, ry), rx.toFloat(), shade)
            canvas.restore()
            // The grains: a dot is a point with round ends.
            val dot = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                strokeCap = Paint.Cap.ROUND
                strokeWidth = grain
                style = Paint.Style.STROKE
            }
            for ((colour, dots) in pile.grains) {
                dot.color = colour
                canvas.drawPoints(dots, dot)
            }
            return bitmap
        }

        private fun draw(context: Context, manager: AppWidgetManager, id: Int) {
            val views = RemoteViews(context.packageName, R.layout.dirt_pile_widget)
            val pile = read(context)
            // Before the app has ever run, there is nothing to draw yet.
            val line = pile?.line ?: context.getString(R.string.dirt_pile_open)
            views.setTextViewText(R.id.dirt_pile_line, line)
            val options = manager.getAppWidgetOptions(id)
            // The room for the heap: the widget less its padding and line.
            val widthDp = max(48, options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 110) - 24)
            val heightDp = max(24, options.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT, 110) - 48)
            val bitmap = pile?.let { runCatching { picture(context, it, widthDp, heightDp) }.getOrNull() }
            if (bitmap != null) {
                views.setImageViewBitmap(R.id.dirt_pile_heap, bitmap)
                views.setViewVisibility(R.id.dirt_pile_heap, View.VISIBLE)
            } else {
                views.setViewVisibility(R.id.dirt_pile_heap, View.GONE)
            }
            views.setContentDescription(R.id.dirt_pile, line)
            context.packageManager.getLaunchIntentForPackage(context.packageName)?.let { app ->
                views.setOnClickPendingIntent(
                    R.id.dirt_pile,
                    PendingIntent.getActivity(
                        context,
                        3,
                        app,
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    ),
                )
            }
            runCatching { manager.updateAppWidget(id, views) }
                .onFailure { failure -> Log.w(TAG, "the pile widget could not be drawn: $failure") }
        }

        // Draws every pile widget again from the file: the app calls this
        // as the person leaves it, and the sheet's jobs once they have
        // taken thoughts in.
        @JvmStatic
        fun refresh(context: Context) {
            val manager = AppWidgetManager.getInstance(context)
            val ids = manager.getAppWidgetIds(ComponentName(context, PileWidget::class.java))
            ids.forEach { id -> draw(context, manager, id) }
        }
    }
}
