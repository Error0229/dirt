package dev.dirt.capture

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.os.Build
import android.util.Log

// The work on this phone after a thought kept in the sheet (design 8.5,
// 6.18, 8.9): make the waiting thoughts and recordings notes, draw the pile
// widget again with their grains, and make the words of what was said. A
// sibling of `CaptureJob`, asked for by the sheet's ✓ and its keeping of a
// recording, and by nothing else: no period, no timer. It needs no
// network, so a thought kept offline is a note, and a grain on the pile,
// at once; nothing is sent. Then it asks for `CaptureJob`, which needs a
// network and syncs once. The speech model is copied out of the APK only
// when a recording waits.
//
// The model takes about 0.6 GiB while it reads, in the app's process.
// One reading at a time in the process, the app's or this job's
// (`voice` in dirt-mobile): never two models at once.
class WordsJob : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        Thread {
            val outcome = synchronized(MAKING) {
                runCatching {
                    // The model is copied out of the APK the first time.
                    val voice = if (Waiting.hasRecordings(applicationContext)) {
                        VoiceFiles.forWords(applicationContext).orEmpty()
                    } else {
                        ""
                    }
                    Core.makeWords(applicationContext, filesDir.absolutePath, voice)
                }
            }
            Log.i(TAG, "words: ${outcome.getOrElse { failure -> "failed: $failure" }}")
            runCatching { PileWidget.refresh(applicationContext) }
            // Whatever was made, the notes are sent once a network is
            // there; what was not made is made by the app when it next
            // starts or comes back, which sends it too.
            runCatching { CaptureJob.schedule(applicationContext) }
                .onFailure { failure -> Log.w(TAG, "the sync could not be asked for: $failure") }
            jobFinished(params, false)
        }.start()
        return true
    }

    // Android stopped the job, its time out. It runs again; a recording
    // being read when it stopped is read again then, with no try counted.
    override fun onStopJob(params: JobParameters): Boolean = true

    companion object {
        private const val JOB = 7402
        private val MAKING = Any()

        // Asks for the job. Asking again while it waits replaces it: one
        // run makes the words of every recording that waits.
        fun schedule(context: Context) {
            val scheduler = context.getSystemService(JobScheduler::class.java)
            fun job(expedited: Boolean): JobInfo {
                val builder = JobInfo.Builder(JOB, ComponentName(context, WordsJob::class.java))
                    // A recording kept just before the phone restarts is
                    // still made a note.
                    .setPersisted(true)
                if (expedited && Build.VERSION.SDK_INT >= 31) builder.setExpedited(true)
                return builder.build()
            }
            val result = runCatching { scheduler.schedule(job(true)) }
                .getOrElse { scheduler.schedule(job(false)) }
            if (result != JobScheduler.RESULT_SUCCESS) scheduler.schedule(job(false))
        }
    }
}
