package dev.dirt.capture

import android.content.Context
import android.os.Build
import android.util.Log
import java.io.File
import java.io.FileOutputStream

// The voice folder (design 6.18, 8.9): the speech model, Silero's detector
// and their notice, shipped in the APK under `assets/voice`. The engine
// opens its files by path, and a file inside the APK has none, so each is
// copied once into the app's folder that is never backed up, and read from
// there. Listening needs only the detector, which is small and copied
// first; the model is copied when words are first made.
//
// A copy is made whole under another name and then renamed, so a file
// there is whole. The folder is marked with the version of the app that
// filled it, and filled again when the app is updated, since the model may
// have changed with it.
object VoiceFiles {
    private const val ASSETS = "voice"
    private const val DETECTOR = "silero_vad.onnx"
    private const val NOTICE = "NOTICE.txt"
    private const val STAMP = ".installed"

    // What turns the sound into words: dirt-voice's `MODEL_FILES`.
    private val MODEL = listOf("encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt")
    private val ONE_AT_A_TIME = Any()

    private fun folder(context: Context) = File(context.noBackupFilesDir, "voice")

    private fun assets(context: Context): Set<String> =
        runCatching { context.assets.list(ASSETS).orEmpty().toSet() }.getOrDefault(emptySet())

    // Whether this build has voice: the detector and the model are among
    // its assets, so what is said can become words. Without them no mic is
    // shown (a build made with `DIRT_VOICE_MODEL=skip` or `DIRT_VOICE=off`).
    @JvmStatic
    fun shipped(context: Context): Boolean {
        val names = assets(context)
        return DETECTOR in names && names.containsAll(MODEL)
    }

    // The folder with the detector in it, for listening, or null in a build
    // without voice. Quick: the detector is under a megabyte.
    @JvmStatic
    fun forListening(context: Context): String? = ready(context, listOf(DETECTOR, NOTICE))

    // The folder with everything in it, for making words: the first time
    // this copies the model, about 130 MiB, which takes a second or more.
    // Never on the main thread.
    @JvmStatic
    fun forWords(context: Context): String? = ready(context, listOf(DETECTOR, NOTICE) + MODEL)

    private fun ready(context: Context, wanted: List<String>): String? = synchronized(ONE_AT_A_TIME) {
        val names = assets(context)
        if (DETECTOR !in names) return null
        val folder = folder(context)
        val installed = installed(context)
        val stamp = File(folder, STAMP)
        if (runCatching { stamp.readText() }.getOrNull() != installed) {
            // Another version of the app filled it, or nothing has.
            folder.deleteRecursively()
            folder.mkdirs()
            if (runCatching { stamp.writeText(installed) }.isFailure) return null
        }
        for (name in wanted.filter { it in names }) {
            if (!copied(context, name, File(folder, name))) return null
        }
        folder.absolutePath
    }

    // This install of the app: its version and when it was installed.
    private fun installed(context: Context): String {
        val info = context.packageManager.getPackageInfo(context.packageName, 0)
        val version = if (Build.VERSION.SDK_INT >= 28) {
            info.longVersionCode
        } else {
            @Suppress("DEPRECATION")
            info.versionCode.toLong()
        }
        return "$version ${info.lastUpdateTime}"
    }

    // Copies one file unless it is there already.
    private fun copied(context: Context, name: String, target: File): Boolean {
        if (target.isFile) return true
        val part = File(target.parentFile, "$name.part")
        return runCatching {
            val began = System.nanoTime()
            context.assets.open("$ASSETS/$name").use { input ->
                FileOutputStream(part).use { output ->
                    input.copyTo(output, 1 shl 20)
                    output.fd.sync()
                }
            }
            check(part.renameTo(target)) { "could not put $name in place" }
            Log.i(TAG, "voice: $name copied out in ${(System.nanoTime() - began) / 1_000_000} ms")
            true
        }.getOrElse { failure ->
            Log.w(TAG, "voice: $name was not copied: $failure")
            part.delete()
            false
        }
    }
}
