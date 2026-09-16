package net.soundpush.service

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioPlaybackCaptureConfiguration
import android.media.AudioRecord
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.annotation.RequiresApi
import net.soundpush.engine.SoundPush
import kotlin.concurrent.thread

/**
 * Android 10+ playback capture. Reads 10 ms blocks of 48 kHz stereo PCM and
 * feeds them to the engine's "app audio" source. Apps that opt out of capture
 * are silent by design.
 */
class AppAudioCapture private constructor(private val projection: MediaProjection, private val record: AudioRecord) {
    @Volatile private var running = true

    private val worker = thread(name = "sp-app-audio", priority = Thread.MAX_PRIORITY) {
        val buffer = ByteArray(960 * 2 * 2) // 10 ms, stereo, 16-bit
        record.startRecording()
        while (running) {
            val n = record.read(buffer, 0, buffer.size, AudioRecord.READ_BLOCKING)
            if (n > 0) {
                SoundPush.direct { pushAppAudioPcm16(if (n == buffer.size) buffer else buffer.copyOf(n)) }
            } else if (n < 0) {
                break
            }
        }
        runCatching { record.stop() }
        record.release()
    }

    fun stop() {
        running = false
        runCatching { projection.stop() }
    }

    companion object {
        @SuppressLint("MissingPermission") // RECORD_AUDIO is checked by the activity before consent.
        fun start(context: Context, resultCode: Int, data: Intent): AppAudioCapture? {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
            return create(context, resultCode, data)
        }

        @RequiresApi(Build.VERSION_CODES.Q)
        @SuppressLint("MissingPermission")
        private fun create(context: Context, resultCode: Int, data: Intent): AppAudioCapture? {
            val manager = context.getSystemService(MediaProjectionManager::class.java) ?: return null
            val projection = runCatching { manager.getMediaProjection(resultCode, data) }.getOrNull() ?: return null
            var capture: AppAudioCapture? = null
            // Android 14+: the projection may be stopped by the system ("Stop sharing").
            projection.registerCallback(
                object : MediaProjection.Callback() {
                    override fun onStop() {
                        capture?.running = false
                        SoundPush.state.value?.routes?.filter { it.kind == "sendAppAudio" }?.forEach { r ->
                            SoundPush.command { stopRoute(r.routeId) }
                        }
                    }
                },
                Handler(Looper.getMainLooper()),
            )

            val config = AudioPlaybackCaptureConfiguration.Builder(projection)
                .addMatchingUsage(AudioAttributes.USAGE_MEDIA)
                .addMatchingUsage(AudioAttributes.USAGE_GAME)
                .addMatchingUsage(AudioAttributes.USAGE_UNKNOWN)
                .build()
            val format = AudioFormat.Builder()
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setSampleRate(48_000)
                .setChannelMask(AudioFormat.CHANNEL_IN_STEREO)
                .build()
            val record = runCatching {
                AudioRecord.Builder()
                    .setAudioFormat(format)
                    .setBufferSizeInBytes(960 * 4 * 4)
                    .setAudioPlaybackCaptureConfig(config)
                    .build()
            }.getOrNull() ?: run {
                projection.stop()
                return null
            }
            capture = AppAudioCapture(projection, record)
            return capture
        }
    }
}
