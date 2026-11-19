package dev.dirt.capture

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.os.Build
import android.util.Log

const val TAG = "DirtCapture"

// The work after the tick (design 8.5): make the waiting thoughts notes,
// and sync once. A job of the platform's JobScheduler, asked for by the
// capture and by nothing else: no period, no timer. It needs a network, so
// a thought kept offline waits for one, which is an event too. If Android
// stops it, or its process dies, Android runs it again; and the app, when
// it starts or the person returns to it, takes in whatever still waits.
class CaptureJob : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        Thread {
            // One run at a time: asking for the job again stops a run
            // Android-side, while its thread carries on. A run waits for
            // the one before it, then takes in what is left.
            val outcome = synchronized(TAKING_IN) {
                runCatching { Core.takeIn(applicationContext, filesDir.absolutePath) }
            }
            Log.i(TAG, "job: ${outcome.getOrElse { failure -> "failed: $failure" }}")
            // Not asked for again on a failure: what was made is in the
            // store, and the next sync from anywhere sends it; what is not
            // made still waits, for the app.
            jobFinished(params, false)
        }.start()
        return true
    }

    // Android stopped the job: the network went, or its time ran out. It
    // runs again once its conditions hold.
    override fun onStopJob(params: JobParameters): Boolean = true

    companion object {
        private const val JOB = 7401
        private val TAKING_IN = Any()

        // Asks for the job. Asking again while it waits replaces it: one
        // run takes in every thought that waits.
        fun schedule(context: Context) {
            val scheduler = context.getSystemService(JobScheduler::class.java)
            fun job(expedited: Boolean): JobInfo {
                val builder = JobInfo.Builder(JOB, ComponentName(context, CaptureJob::class.java))
                    .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                    // Kept across a restart of the phone: a thought kept
                    // offline is not left waiting for the app.
                    .setPersisted(true)
                if (expedited && Build.VERSION.SDK_INT >= 31) builder.setExpedited(true)
                return builder.build()
            }
            // Past the app's quota of expedited jobs, an ordinary one, which
            // Android may put off.
            val result = runCatching { scheduler.schedule(job(true)) }
                .getOrElse { scheduler.schedule(job(false)) }
            if (result != JobScheduler.RESULT_SUCCESS) scheduler.schedule(job(false))
        }
    }
}
