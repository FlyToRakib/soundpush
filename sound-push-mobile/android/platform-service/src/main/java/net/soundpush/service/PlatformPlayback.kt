package net.soundpush.service

import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTimestamp
import android.media.AudioTrack
import android.media.audiofx.AudioEffect
import android.os.SystemClock
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.SoundPush
import kotlin.concurrent.thread

/**
 * Platform playback path (AudioRelay's "AudioTrack" output). Pulls mixed 48 kHz stereo PCM from
 * the engine and plays it through a regular AudioTrack, so the device's own audio effects and
 * equalizer apps apply. Used when the user turns on compatibility output or output audio effects,
 * and automatically when the low-latency path fails to open.
 *
 * [legacy] uses the old stream-type constructor, which works on devices whose attribute-based
 * routing is broken; otherwise media attributes without the low-latency request.
 */
class PlatformPlayback private constructor(private val context: Context, private val track: AudioTrack) {
    @Volatile private var running = true

    private val worker = thread(name = "sp-playback", priority = Thread.MAX_PRIORITY) {
        val chunk = FRAMES_PER_PULL.toUInt()
        var written = 0L
        var lastLatencyCheck = 0L
        val timestamp = AudioTimestamp()
        openEffectSession(true)
        runCatching { track.play() }
        while (running) {
            val pcm = SoundPush.direct { pullPlaybackPcm16(chunk) } ?: break
            // Blocking write paces the pulls to what the device actually consumes.
            val n = track.write(pcm, 0, pcm.size, AudioTrack.WRITE_BLOCKING)
            if (n < 0) break
            written += n / BYTES_PER_FRAME
            val now = SystemClock.elapsedRealtime()
            if (now - lastLatencyCheck >= 1_000 && track.getTimestamp(timestamp)) {
                lastLatencyCheck = now
                // Frames written but not yet heard, minus the time since that frame was presented.
                val pendingMs = (written - timestamp.framePosition) * 1000 / SAMPLE_RATE
                val sinceMs = (System.nanoTime() - timestamp.nanoTime) / 1_000_000
                DeviceStatus.reportOutputLatency((pendingMs - sinceMs).coerceAtLeast(0).toInt())
            }
        }
        runCatching { track.pause() }
        runCatching { track.flush() }
        openEffectSession(false)
        track.release()
        DeviceStatus.reportOutputLatency(0)
    }

    fun stop() {
        running = false
    }

    /** Lets equalizer apps (and the system's sound effects screen) attach to this session. */
    private fun openEffectSession(open: Boolean) {
        val action = if (open) AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION else AudioEffect.ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION
        runCatching {
            context.sendBroadcast(
                Intent(action)
                    .putExtra(AudioEffect.EXTRA_AUDIO_SESSION, track.audioSessionId)
                    .putExtra(AudioEffect.EXTRA_PACKAGE_NAME, context.packageName)
                    .putExtra(AudioEffect.EXTRA_CONTENT_TYPE, AudioEffect.CONTENT_TYPE_MUSIC),
            )
        }
    }

    companion object {
        private const val SAMPLE_RATE = 48_000
        private const val BYTES_PER_FRAME = 4
        private const val FRAMES_PER_PULL = 480 // 10 ms

        fun start(context: Context, legacy: Boolean): PlatformPlayback? {
            val minBuffer = AudioTrack.getMinBufferSize(SAMPLE_RATE, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_16BIT)
            val bufferBytes = maxOf(minBuffer, FRAMES_PER_PULL * BYTES_PER_FRAME * 4)
            val track = runCatching {
                if (legacy) {
                    @Suppress("DEPRECATION")
                    AudioTrack(
                        AudioManager.STREAM_MUSIC,
                        SAMPLE_RATE,
                        AudioFormat.CHANNEL_OUT_STEREO,
                        AudioFormat.ENCODING_PCM_16BIT,
                        bufferBytes,
                        AudioTrack.MODE_STREAM,
                    )
                } else {
                    AudioTrack.Builder()
                        .setAudioAttributes(
                            AudioAttributes.Builder()
                                .setUsage(AudioAttributes.USAGE_MEDIA)
                                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                                .build(),
                        )
                        .setAudioFormat(
                            AudioFormat.Builder()
                                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                                .setSampleRate(SAMPLE_RATE)
                                .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                                .build(),
                        )
                        .setBufferSizeInBytes(bufferBytes)
                        // No low-latency request: the regular mixer path is where device effects run.
                        .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_NONE)
                        .setTransferMode(AudioTrack.MODE_STREAM)
                        .build()
                }
            }.getOrNull()?.takeIf { it.state == AudioTrack.STATE_INITIALIZED } ?: return null
            return PlatformPlayback(context.applicationContext, track)
        }
    }
}
