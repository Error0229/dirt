package dev.dirt.capture

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.os.SystemClock
import android.provider.Settings
import android.view.View
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin

// The row of grains that moves with the voice while the sheet listens
// (design 6.18): `dirt-ui`'s, drawn here because the sheet has no web view.
// Like sand on a drum: still in silence, most in the middle, higher as it
// gets louder, eased so that it does not flicker. With "Remove animations"
// the grains stay in a still row, and only their strength changes.
class GrainsView(context: Context) : View(context) {
    // How loud it is now, from 0 to 1, as `Loudness` gives it.
    var level = 0f

    private var shown = 0f
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val still = Settings.Global.getFloat(
        context.contentResolver,
        Settings.Global.ANIMATOR_DURATION_SCALE,
        1f,
    ) == 0f
    private val sand = IntArray(9) { index -> shade(index) }
    private val density = resources.displayMetrics.density

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        shown += (level - shown) * 0.2f
        val w = width / density
        val h = height / density
        if (w > 10) {
            val count = max(8, floor(w / 10).toInt())
            val t = SystemClock.uptimeMillis() / 1000.0
            for (index in 0 until count) {
                val middle = sin(PI * (index + 0.5) / count).toFloat()
                val height = if (still) 0f else shown * middle * h * 0.4f
                val beat = (sin(t * 9 + index * 0.9) * 0.6 + sin(t * 13.7 + index * 2.3) * 0.4).toFloat()
                paint.color = sand[index % sand.size]
                paint.alpha = (255 * (0.45f + 0.55f * min(1f, shown * middle * 1.6f + 0.15f))).toInt()
                val x = 5 + index * (w - 10) / (count - 1)
                val y = h / 2 + beat * height
                val r = 2.3f + shown * middle * 1.4f
                canvas.drawCircle(x * density, y * density, r * density, paint)
            }
        }
        if (isAttachedToWindow && visibility == VISIBLE) postInvalidateOnAnimation()
    }

    private companion object {
        // Nine of the pile's shades, made as `dirt-ui`'s voice row makes
        // them from the same seeds.
        fun shade(index: Int): Int {
            val seed = (index.toLong() * 0x9e3779b9L xor 0x5bd1e995L) and 0xffffffffL
            val hue = 20 + (seed % 24).toInt()
            val saturation = 30 + ((seed shr 5) % 32).toInt()
            val lightness = 48 + ((seed shr 10) % 24).toInt()
            return hsl(hue.toFloat(), saturation / 100f, lightness / 100f)
        }

        fun hsl(hue: Float, saturation: Float, lightness: Float): Int {
            val chroma = (1 - abs(2 * lightness - 1)) * saturation
            val part = hue / 60f
            val x = chroma * (1 - abs(part % 2 - 1))
            val (r, g, b) = when {
                part < 1 -> Triple(chroma, x, 0f)
                part < 2 -> Triple(x, chroma, 0f)
                else -> Triple(0f, chroma, x)
            }
            val m = lightness - chroma / 2
            fun byte(value: Float) = ((value + m) * 255).toInt().coerceIn(0, 255)
            return (0xFF shl 24) or (byte(r) shl 16) or (byte(g) shl 8) or byte(b)
        }
    }
}
