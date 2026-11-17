package dev.dirt.capture

import android.app.Application
import android.app.KeyguardManager
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.os.Build
import android.os.PersistableBundle
import android.os.Process
import android.os.SystemClock
import android.util.Log

const val TAG = "DirtCapture"

fun processName(): String =
    if (Build.VERSION.SDK_INT >= 28) Application.getProcessName() else "pid ${Process.myPid()}"

// TRIAL 2. The work after the tick: make the waiting thoughts notes, sync
// once. A job of the platform's JobScheduler, asked for by the capture and
// by nothing else: no period, no timer. It needs a network, so a thought
// captured offline is synced when the phone is next online, which is an
// event too.
open class CaptureJobService : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        val extras = params.extras
        val waited = SystemClock.elapsedRealtime() - extras.getLong(EXTRA_SCHEDULED_AT, 0)
        val locked = getSystemService(KeyguardManager::class.java)
        Log.i(
            TAG,
            "job start process=${processName()} pid=${Process.myPid()} " +
                "waited=${waited}ms keyguardLocked=${locked.isKeyguardLocked} " +
                "deviceLocked=${locked.isDeviceLocked} waiting=${Waiting.count(this)}",
        )
        Thread {
            val outcome = runCatching {
                val before = SystemClock.elapsedRealtime()
                Core.nothing()
                Log.i(TAG, "job library ready in ${SystemClock.elapsedRealtime() - before}ms")
                val pause = extras.getInt(EXTRA_PAUSE_MS, 0).toLong()
                when (extras.getString(EXTRA_PAUSE_AT)) {
                    // For killing the process before the note is made.
                    "before" -> {
                        Log.i(TAG, "job pausing ${pause}ms before taking in")
                        Thread.sleep(pause)
                    }
                    // For killing it with the note made and not yet synced.
                    "between" -> {
                        val made = Core.takeIn(applicationContext, filesDir.absolutePath, false)
                        Log.i(TAG, "job took in, no sync yet: $made")
                        Log.i(TAG, "job pausing ${pause}ms before syncing")
                        Thread.sleep(pause)
                    }
                }
                Core.takeIn(applicationContext, filesDir.absolutePath, true)
            }
            Log.i(TAG, "job done: ${outcome.getOrElse { failure -> "failed: $failure" }}")
            // Not asked for again on failure: the note is in the store, and
            // the next sync from anywhere sends it.
            jobFinished(params, false)
        }.start()
        return true
    }

    // The system stopped the job: the network went, or its time ran out.
    // Asked for again, it runs when its conditions hold again.
    override fun onStopJob(params: JobParameters): Boolean {
        val why = if (Build.VERSION.SDK_INT >= 31) params.stopReason.toString() else "?"
        Log.i(TAG, "job stopped by the system, reason=$why")
        return true
    }

    companion object {
        const val EXTRA_SCHEDULED_AT = "scheduled_at"
        const val EXTRA_PAUSE_MS = "pause_ms"
        const val EXTRA_PAUSE_AT = "pause_at"
        private const val JOB = 7401
        private const val JOB_OWN = 7402

        fun schedule(
            context: Context,
            ownProcess: Boolean,
            expedited: Boolean,
            pauseAt: String?,
            pauseMs: Int,
        ): Boolean {
            val service = ComponentName(
                context,
                if (ownProcess) CaptureJobServiceOwn::class.java else CaptureJobService::class.java,
            )
            val extras = PersistableBundle().apply {
                putLong(EXTRA_SCHEDULED_AT, SystemClock.elapsedRealtime())
                if (pauseAt != null) {
                    putString(EXTRA_PAUSE_AT, pauseAt)
                    putInt(EXTRA_PAUSE_MS, pauseMs)
                }
            }
            val job = JobInfo.Builder(if (ownProcess) JOB_OWN else JOB, service)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .setExtras(extras)
            if (expedited && Build.VERSION.SDK_INT >= 31) job.setExpedited(true)
            val scheduler = context.getSystemService(JobScheduler::class.java)
            val result = scheduler.schedule(job.build())
            Log.i(TAG, "job scheduled result=$result expedited=$expedited own=$ownProcess")
            return result == JobScheduler.RESULT_SUCCESS
        }
    }
}

// The same job in the `:capture` process, to compare.
class CaptureJobServiceOwn : CaptureJobService()
