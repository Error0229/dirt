package dev.dirt.capture

import android.content.Context
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.security.SecureRandom
import java.util.UUID

// TRIAL 2. Where a thought waits between the tick and becoming a note: a
// file of its own in the app's private storage, named by the note's ID,
// which is made here, at the moment of capture.
//
//     <files>/dirt/waiting/<UUID v7>.txt
//
// The Rust side (`capture::take_in`) turns each into a note under that ID
// and removes the file. The ID carries the time of capture, so the note's
// creation time is right however long the file waited, and taking the same
// file in twice makes one note.
object Waiting {
    private val random = SecureRandom()

    private fun dataFolder(context: Context) = File(context.filesDir, "dirt")

    fun folder(context: Context) = File(dataFolder(context), "waiting")

    // A version 7 UUID: 48 bits of Unix milliseconds, then random bits,
    // with the version and variant set (RFC 9562). It is what `dirt-core`
    // makes with `Uuid::now_v7`, and reads a note's creation time from.
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
    // the ID the note will have.
    fun keep(context: Context, text: String): String {
        val folder = folder(context)
        folder.mkdirs()
        val id = newId()
        val unfinished = File(folder, "$id.tmp")
        FileOutputStream(unfinished).use { file ->
            file.write(text.toByteArray(Charsets.UTF_8))
            file.fd.sync()
        }
        if (!unfinished.renameTo(File(folder, "$id.txt"))) error("could not keep the thought")
        return id
    }

    // How many thoughts wait. The pile widget would add this to the count
    // the app last wrote down, to draw the new grain before the app runs.
    fun count(context: Context): Int =
        folder(context).listFiles { file -> file.name.endsWith(".txt") }?.size ?: 0

    // What was written and not kept, for the next time the sheet opens.
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
