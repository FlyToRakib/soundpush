package net.soundpush.app

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Process
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineJson
import net.soundpush.engine.EngineState
import net.soundpush.engine.SoundPush
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * Diagnostics bundle, like the desktop's export: app/OS/device info, the engine state with remote
 * addresses and pairing secrets removed, local crash reports and this app's recent log. Written
 * to the app's cache and handed to the share sheet; nothing leaves the phone unless the user shares it.
 */
object Diagnostics {
    private const val LOG_LINES = 1000
    private const val LOG_BYTES = 512L * 1024

    /** An export older than this is certainly not being read by the app the user shared it with. */
    private val EXPORT_KEEP_MS = TimeUnit.MINUTES.toMillis(10)

    /** Build the report file. Reads the log and files: runs on the IO dispatcher. */
    suspend fun export(context: Context): Uri? = withContext(Dispatchers.IO) {
        runCatching {
            val dir = File(context.cacheDir, "diagnostics").apply { mkdirs() }
            // Keep only the newest export around.
            dir.listFiles()?.forEach { it.delete() }
            val file = File(dir, "soundpush-diagnostics-${System.currentTimeMillis() / 1000}.txt")
            file.writeText(report(context, SoundPush.state.value))
            FileProvider.getUriForFile(context, "${context.packageName}.files", file)
        }.getOrNull()
    }

    /**
     * Drop exports left in the cache when Android is short of memory (plan §8.4). A just-shared
     * file is left alone: the app it went to may still be reading it through the content URI.
     */
    fun clearOldExports(context: Context) {
        val stale = System.currentTimeMillis() - EXPORT_KEEP_MS
        File(context.cacheDir, "diagnostics").listFiles()?.forEach { file ->
            if (file.lastModified() < stale) file.delete()
        }
    }

    fun share(context: Context, uri: Uri) {
        val send = Intent(Intent.ACTION_SEND)
            .setType("text/plain")
            .putExtra(Intent.EXTRA_STREAM, uri)
            .putExtra(Intent.EXTRA_SUBJECT, "SoundPush diagnostics")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        send.clipData = ClipData.newRawUri(null, uri)
        runCatching { context.startActivity(Intent.createChooser(send, null)) }
    }

    private fun report(context: Context, state: EngineState?): String = buildString {
        appendLine("SoundPush ${BuildVersion.NAME} diagnostics (Android)")
        appendLine("Generated: ${System.currentTimeMillis() / 1000}")
        appendLine("Device: ${Build.MANUFACTURER} ${Build.MODEL} (${Build.DEVICE})")
        appendLine("Android: ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}), ABI ${Build.SUPPORTED_ABIS.joinToString()}")
        appendLine("Engine: ${if (SoundPush.isStarted) "running" else "not started"}")
        appendLine("Audio output: ${DeviceStatus.output.value}, platform player latency ${DeviceStatus.outputLatencyMs.value} ms")
        val net = DeviceStatus.network.value
        appendLine(
            "Network: wifi=${net.wifi} ethernet=${net.ethernet} cellular=${net.cellular} vpn=${net.vpn} " +
                "usbTethering=${net.usbTethering} hotspot=${net.hotspot}",
        )
        appendLine()
        appendLine("== State ==")
        appendLine(state?.let { EngineJson.encodeToString(EngineState.serializer(), redact(it)) } ?: "(no state)")
        appendLine()
        appendLine("== Crash reports ==")
        appendLine(CrashReports.collect(context))
        appendLine()
        appendLine("== Engine and app log ==")
        appendLine(engineLog(context))
        appendLine()
        appendLine("== Recent logcat ==")
        appendLine(recentLog())
    }

    /**
     * The engine's log files (app messages go through the engine logger too), oldest first, at most
     * the last [LOG_BYTES] of each. Rotated by the engine at 3 × 2 MB.
     */
    private fun engineLog(context: Context): String = runCatching {
        val dir = File(context.filesDir, "logs")
        val files = listOf("soundpush.2.log", "soundpush.1.log", "soundpush.log").map { File(dir, it) }.filter { it.isFile }
        if (files.isEmpty()) return@runCatching "(no log files)"
        files.joinToString("\n") { file ->
            java.io.RandomAccessFile(file, "r").use { raf ->
                val start = (raf.length() - LOG_BYTES).coerceAtLeast(0)
                raf.seek(start)
                val bytes = ByteArray((raf.length() - start).toInt())
                raf.readFully(bytes)
                "--- ${file.name}\n" + String(bytes, Charsets.UTF_8)
            }
        }
    }.getOrElse { "(log unavailable: ${it.javaClass.simpleName})" }

    /** Same redaction as the desktop export, plus the pairing QR (it carries a one-time secret). */
    internal fun redact(state: EngineState): EngineState = state.copy(
        peers = state.peers.map { p -> p.copy(deviceId = p.deviceId.take(8), addresses = p.addresses.map { "<redacted>" }) },
        pairing = state.pairing.copy(qrUri = state.pairing.qrUri?.let { "<redacted>" }),
    )

    /** This process's own log (the engine logs here too). No permission needed for our own pid. */
    private fun recentLog(): String = runCatching {
        val process = ProcessBuilder("logcat", "-d", "-v", "threadtime", "-t", LOG_LINES.toString(), "--pid=${Process.myPid()}")
            .redirectErrorStream(true)
            .start()
        val text = process.inputStream.bufferedReader().use { it.readText() }
        if (!process.waitFor(5, TimeUnit.SECONDS)) process.destroy()
        text
    }.getOrElse { "(log unavailable: ${it.javaClass.simpleName})" }
}
