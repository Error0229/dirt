package dev.dirt.capture

import android.Manifest
import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.SystemClock
import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.log10
import kotlin.math.max

// The phone's microphone (design 6.18, 8.9): Android's AudioRecord at
// 16 kHz, one channel, 16 bits, what dirt-voice works at. It is opened by a
// press and closed at its end, and nothing listens otherwise.
//
// Two listen: the app's Rust, which is handed each piece as it comes
// (`startForApp`, `heard`), and the capture sheet, which has none of the
// app's Rust and writes the sound to a file of its own (`WavWriter`).
//
// A debuggable build with `dirt/stand-in.wav` among the app's files plays
// that instead, at the pace it was recorded, and then a quiet room: for
// trying voice on an emulator, whose microphone is the computer's.
class Listener(private val context: Context, private val onSound: (ShortArray, Int) -> Unit) {
    @Volatile private var running = false
    private var thread: Thread? = null

    // Starts listening. False when the microphone could not be opened;
    // a SecurityException without the permission.
    fun start(): Boolean {
        running = true
        standIn(context)?.let { wav ->
            thread = Thread({ playStandIn(wav) }, "dirt-microphone").apply { start() }
            return true
        }
        val least = AudioRecord.getMinBufferSize(RATE, CHANNELS, ENCODING)
        if (least <= 0) return false
        val record = AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION,
            RATE,
            CHANNELS,
            ENCODING,
            max(least, RATE / 5 * 2),
        )
        if (record.state != AudioRecord.STATE_INITIALIZED) {
            record.release()
            return false
        }
        record.startRecording()
        if (record.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
            record.release()
            return false
        }
        thread = Thread({
            val piece = ShortArray(RATE / 10)
            while (running) {
                val read = record.read(piece, 0, piece.size)
                if (read > 0) onSound(piece, read) else if (read < 0) break
            }
            // What was recorded before the stop is kept.
            record.stop()
            while (true) {
                val read = record.read(piece, 0, piece.size)
                if (read <= 0) break
                onSound(piece, read)
            }
            record.release()
        }, "dirt-microphone").apply { start() }
        return true
    }

    // Stops, and returns once every piece heard has been handed on.
    fun stop() {
        running = false
        thread?.join()
        thread = null
    }

    private fun playStandIn(wav: File) {
        val samples = runCatching { readWav(wav) }.getOrDefault(ShortArray(0))
        val began = SystemClock.uptimeMillis()
        var index = 0
        var at = 0
        val piece = ShortArray(RATE / 10)
        var noise = 0x2545f491
        while (running) {
            val have = samples.size - at
            if (have > 0) {
                val count = minOf(piece.size, have)
                System.arraycopy(samples, at, piece, 0, count)
                at += count
                onSound(piece, count)
            } else {
                for (i in piece.indices) {
                    noise = noise * 1664525 + 1013904223
                    piece[i] = ((noise ushr 24) % 7 - 3).toShort()
                }
                onSound(piece, piece.size)
            }
            index += 1
            val wait = began + 100L * index - SystemClock.uptimeMillis()
            if (wait > 0) Thread.sleep(wait)
        }
    }

    companion object {
        const val RATE = 16000
        private const val CHANNELS = AudioFormat.CHANNEL_IN_MONO
        private const val ENCODING = AudioFormat.ENCODING_PCM_16BIT

        private fun standIn(context: Context): File? {
            val debuggable = context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
            val file = File(Waiting.dataFolder(context), "stand-in.wav")
            return if (debuggable && file.isFile) file else null
        }

        // A WAV of 16 kHz, one channel, 16 bits: its samples.
        private fun readWav(file: File): ShortArray {
            val bytes = file.readBytes()
            val all = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            var at = 12
            while (at + 8 <= bytes.size) {
                val id = String(bytes, at, 4, Charsets.US_ASCII)
                val size = all.getInt(at + 4)
                if (id == "data") {
                    val count = minOf(size, bytes.size - at - 8) / 2
                    val out = ShortArray(count)
                    all.position(at + 8)
                    all.asShortBuffer().get(out)
                    return out
                }
                at += 8 + size + (size and 1)
            }
            return ShortArray(0)
        }
    }
}

// How loud a piece is, and what was heard: speech, and the quiet after it.
// The sheet has none of the app's Rust, so not Silero's detector. Speech is
// judged against the room: the noise floor is the quietest tenth of a
// second of the last three, so steady noise (a car, the street) is the
// floor and not speech, and a soft voice in a quiet room is still above
// it. A piece 9 dB over the floor is speech, and one within 6 dB of it is
// quiet; nothing under -65 dBFS is speech, however quiet the room. This is
// what the grains, the sheet's shake and the end of a Talk go by. Whether
// anything was said is Silero's to say, when the app's Rust takes the
// recording in: the sheet keeps every recording the person kept.
class Loudness {
    var speechMs = 0L
        private set
    private var quietSince = -1L
    var heardMs = 0L
        private set
    private val recent = ArrayDeque<Double>()

    // The level of the piece, from 0 to 1, as the grains want it.
    fun take(piece: ShortArray, count: Int): Float {
        var sum = 0.0
        for (i in 0 until count) {
            val value = piece[i] / 32768.0
            sum += value * value
        }
        val db = 10 * log10(max(sum / max(count, 1), 1e-10))
        val pieceMs = count * 1000L / Listener.RATE
        heardMs += pieceMs
        recent.addLast(db)
        while (recent.size > FLOOR_PIECES) recent.removeFirst()
        val floor = recent.min()
        if (db > max(floor + SPEECH_OVER_DB, LEAST_SPEECH_DB)) {
            speechMs += pieceMs
            quietSince = -1
        } else if (db <= floor + QUIET_WITHIN_DB) {
            if (quietSince < 0) quietSince = heardMs - pieceMs
        }
        return ((db - QUIETEST_DB) / (LOUDEST_DB - QUIETEST_DB)).toFloat().coerceIn(0f, 1f)
    }

    // Speech was heard: a third of a second of it.
    val speech get() = speechMs >= 300

    // How long it has been quiet since speech, in milliseconds.
    val quietAfterSpeechMs get() = if (!speech || quietSince < 0) 0L else heardMs - quietSince

    private companion object {
        // Three seconds of pieces of a tenth of a second.
        const val FLOOR_PIECES = 30
        const val SPEECH_OVER_DB = 9.0
        const val QUIET_WITHIN_DB = 6.0
        const val LEAST_SPEECH_DB = -65.0
        const val QUIETEST_DB = -55.0
        const val LOUDEST_DB = -15.0
    }
}

// The sound as a WAV file, written as it comes and finished at the end:
// 16 kHz, one channel, 16 bits. The app's Rust takes it in, writes it as
// Ogg Opus on the note, and removes it.
//
// The header is written whole before the first sample, with its lengths
// nought, and written again with them at the end. A recording whose
// process ends while it listens is then still a WAV, whose lengths say
// nothing, and the app's Rust reads it to its end (`capture::samples_of_wav`,
// tested on bytes this wrote on the emulator).
class WavWriter(private val file: File) {
    private val out = RandomAccessFile(file, "rw")
    private var bytes = 0L

    init {
        out.setLength(0)
        out.write(header(0))
    }

    private fun header(data: Long): ByteArray {
        val head = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
        head.put("RIFF".toByteArray(Charsets.US_ASCII)).putInt(if (data == 0L) 0 else (36 + data).toInt())
        head.put("WAVEfmt ".toByteArray(Charsets.US_ASCII)).putInt(16)
        head.putShort(1).putShort(1).putInt(Listener.RATE).putInt(Listener.RATE * 2)
        head.putShort(2).putShort(16)
        head.put("data".toByteArray(Charsets.US_ASCII)).putInt(data.toInt())
        return head.array()
    }

    fun write(piece: ShortArray, count: Int) {
        val buffer = ByteBuffer.allocate(count * 2).order(ByteOrder.LITTLE_ENDIAN)
        for (i in 0 until count) buffer.putShort(piece[i])
        out.write(buffer.array())
        bytes += count * 2
    }

    val durationMs get() = bytes / 2 * 1000 / Listener.RATE

    fun finish() {
        out.seek(0)
        out.write(header(bytes))
        out.fd.sync()
        out.close()
    }

    fun throwAway() {
        runCatching { out.close() }
        file.delete()
    }
}

// The app's Rust listening: `dirt-mobile`'s `voice` calls these through
// JNI, and is handed the sound through `heard`, which it exports.
object Microphone {
    private var forApp: Listener? = null

    @JvmStatic
    external fun heard(samples: ShortArray, count: Int)

    // 0: listening; 1: the app may not use the microphone; 2: it could not
    // be opened.
    @JvmStatic
    @Synchronized
    fun startForApp(context: Context): Int {
        if (context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            return 1
        }
        forApp?.stop()
        val listener = Listener(context) { piece, count -> heard(piece, count) }
        return runCatching { if (listener.start()) 0 else 2 }
            .getOrElse { failure -> if (failure is SecurityException) 1 else 2 }
            .also { answer -> forApp = if (answer == 0) listener else null }
    }

    // Stops, and returns once every piece heard has been handed on.
    @JvmStatic
    @Synchronized
    fun stopForApp() {
        forApp?.stop()
        forApp = null
    }
}
