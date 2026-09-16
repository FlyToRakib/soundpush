package net.soundpush.app

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import androidx.annotation.RequiresApi
import java.io.File
import java.io.PrintWriter
import java.io.StringWriter
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Local crash reports. A Kotlin crash writes a small report file before the process dies; native
 * crashes and ANRs come from Android's own exit records (Android 11+). Reports stay on the phone:
 * they are shown once on the next launch and included in the diagnostics export, nothing more.
 */
object CrashReports {
    private const val MAX_FILES = 5
    private const val PREFS = "soundpush_local"
    private const val KEY_SEEN_AT = "crashSeenAt"

    fun install(context: Context) {
        val app = context.applicationContext
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching { write(app, thread, error) }
            previous?.uncaughtException(thread, error)
        }
    }

    private fun dir(context: Context) = File(context.filesDir, "crashes")

    private fun write(context: Context, thread: Thread, error: Throwable) {
        val dir = dir(context).apply { mkdirs() }
        val trace = StringWriter().also { error.printStackTrace(PrintWriter(it)) }.toString()
        val report = buildString {
            appendLine("SoundPush ${BuildVersion.NAME} crash")
            appendLine("Time: ${timestamp(System.currentTimeMillis())}")
            appendLine("Device: ${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
            appendLine("Thread: ${thread.name}")
            appendLine()
            append(trace.take(64_000))
        }
        File(dir, "crash-${System.currentTimeMillis()}.txt").writeText(report)
        dir.listFiles()?.sortedByDescending { it.lastModified() }?.drop(MAX_FILES)?.forEach { it.delete() }
    }

    /** Whether a crash happened since the user last saw the notice. Reads files: call off the main thread. */
    fun hasUnseen(context: Context): Boolean {
        val seenAt = prefs(context).getLong(KEY_SEEN_AT, 0L)
        val fileNewer = dir(context).listFiles()?.any { it.lastModified() > seenAt } == true
        return fileNewer || exitRecords(context).any { it.first > seenAt }
    }

    fun markSeen(context: Context) {
        prefs(context).edit().putLong(KEY_SEEN_AT, System.currentTimeMillis()).apply()
    }

    /** Every stored report and recent abnormal exit, for the diagnostics bundle. */
    fun collect(context: Context): String = buildString {
        val files = dir(context).listFiles()?.sortedByDescending { it.lastModified() }.orEmpty()
        if (files.isEmpty()) appendLine("No crash reports.")
        files.forEach { f ->
            appendLine("--- ${f.name}")
            appendLine(runCatching { f.readText() }.getOrDefault("(unreadable)"))
        }
        val exits = exitRecords(context)
        if (exits.isNotEmpty()) {
            appendLine()
            appendLine("Recent abnormal exits (Android):")
            exits.forEach { (time, text) -> appendLine("${timestamp(time)} $text") }
        }
    }

    private fun exitRecords(context: Context): List<Pair<Long, String>> = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) exitRecordsR(context) else emptyList()

    @RequiresApi(Build.VERSION_CODES.R)
    private fun exitRecordsR(context: Context): List<Pair<Long, String>> {
        val am = context.getSystemService(ActivityManager::class.java) ?: return emptyList()
        val infos = runCatching { am.getHistoricalProcessExitReasons(context.packageName, 0, 10) }.getOrNull().orEmpty()
        return infos
            .filter { it.reason in setOf(ApplicationExitInfo.REASON_CRASH, ApplicationExitInfo.REASON_CRASH_NATIVE, ApplicationExitInfo.REASON_ANR) }
            .map { info ->
                val kind = when (info.reason) {
                    ApplicationExitInfo.REASON_CRASH_NATIVE -> "native crash"
                    ApplicationExitInfo.REASON_ANR -> "not responding"
                    else -> "crash"
                }
                // Native tombstones and ANR traces can be large: keep a short summary.
                val trace = runCatching {
                    info.traceInputStream?.bufferedReader()?.useLines { lines -> lines.take(80).joinToString("\n") }
                }.getOrNull()
                info.timestamp to buildString {
                    append(kind)
                    info.description?.let { append(": ").append(it) }
                    if (!trace.isNullOrBlank()) append('\n').append(trace)
                }
            }
    }

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun timestamp(millis: Long) = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date(millis))
}
