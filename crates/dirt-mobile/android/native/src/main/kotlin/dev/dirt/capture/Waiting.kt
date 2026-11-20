package dev.dirt.capture

import android.content.Context
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.security.SecureRandom
import java.util.UUID

// Where a thought waits between the tick and becoming a note (design 8.5): a
// file of its own in the app's private storage, named by the note's ID,
// which is made here, at the moment of capture.
//
//     <files>/dirt/waiting/<UUID v7>.txt
//
// The Rust side (`capture::take_in` in dirt-mobile) makes each one a note
// under that ID and removes the file. The ID carries the time of capture,
// so the note is dated when it was written however long the file waited,
// and taking the same file in twice makes one note.
object Waiting {
    private val random = SecureRandom()

    // The app's data folder: the Rust side's, under the app's files.
    fun dataFolder(context: Context) = File(context.filesDir, "dirt")

    private fun folder(context: Context) = File(dataFolder(context), "waiting")

    // A version 7 UUID: 48 bits of Unix milliseconds, then random bits, with
    // the version and the variant set (RFC 9562). It is what dirt-core makes
    // with `Uuid::now_v7`, and reads a note's creation time from.
    fun newId(nowMs: Long = System.currentTimeMillis()): String {
        val bytes = ByteArray(16)
        random.nextBytes(bytes)
        for (index in 0..5) bytes[index] = (nowMs shr (8 * (5 - index))).toByte()
        bytes[6] = ((bytes[6].toInt() and 0x0F) or 0x70).toByte()
        bytes[8] = ((bytes[8].toInt() and 0x3F) or 0x80).toByte()
        val both = ByteBuffer.wrap(bytes)
        return UUID(both.long, both.long).toString()
    }

    // Keeps a thought: written whole and flushed to the disk under another
    // name, then renamed, so that a `.txt` is never half a thought. Returns
    // the ID the note will have. Throws if it could not be kept.
    fun keep(context: Context, text: String): String {
        val folder = folder(context)
        folder.mkdirs()
        val id = newId()
        val unfinished = File(folder, "$id.tmp")
        FileOutputStream(unfinished).use { file ->
            file.write(text.toByteArray(Charsets.UTF_8))
            file.fd.sync()
        }
        if (!unfinished.renameTo(File(folder, "$id.txt"))) {
            unfinished.delete()
            error("the thought could not be kept")
        }
        return id
    }

    // A recording from the sheet's mic waits the same way, as a WAV beside
    // the typed thoughts (design 8.9): written as it is heard to
    // `<id>.wav.part`, and renamed `<id>.wav` once it is finished, so that
    // a `.wav` is whole. The ID is made as listening starts: the note is
    // dated when the thought began.
    fun newRecording(context: Context): Pair<String, File> {
        val folder = folder(context)
        folder.mkdirs()
        val id = newId()
        return id to File(folder, "$id.wav.part")
    }

    // The recording is finished: it waits whole, for the job or the app.
    fun keepRecording(part: File): Boolean {
        val whole = File(part.parentFile, part.name.removeSuffix(".part"))
        return part.renameTo(whole)
    }

    // What was written in the sheet and not kept, for the next time it opens
    // on an unlocked phone.
    private fun draftFile(context: Context) = File(dataFolder(context), "capture-draft.txt")

    fun draft(context: Context): String =
        runCatching { draftFile(context).readText(Charsets.UTF_8) }.getOrDefault("")

    fun setDraft(context: Context, text: String) {
        val file = draftFile(context)
        if (text.isEmpty()) {
            file.delete()
        } else {
            file.parentFile?.mkdirs()
            file.writeText(text, Charsets.UTF_8)
        }
    }
}
