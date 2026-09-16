package net.soundpush.service

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.media.audiofx.AcousticEchoCanceler
import android.media.audiofx.AudioEffect
import android.media.audiofx.AutomaticGainControl
import android.media.audiofx.NoiseSuppressor
import android.os.Build
import net.soundpush.engine.MicSettings
import net.soundpush.engine.SoundPush
import kotlin.concurrent.thread

/**
 * Microphone capture with the user's input preset (AudioRelay's seven modes)
 * and platform effects, fed to the engine as 48 kHz mono PCM in 10 ms blocks.
 */
class MicCapture private constructor(private val record: AudioRecord, private val effects: List<AudioEffect>) {
    @Volatile private var running = true

    private val worker = thread(name = "sp-mic", priority = Thread.MAX_PRIORITY) {
        val buffer = ByteArray(480 * 2)
        record.startRecording()
        while (running) {
            val n = record.read(buffer, 0, buffer.size, AudioRecord.READ_BLOCKING)
            if (n > 0) {
                SoundPush.direct { pushMicPcm16(if (n == buffer.size) buffer else buffer.copyOf(n)) }
            } else if (n < 0) {
                break
            }
        }
        runCatching { record.stop() }
        effects.forEach { runCatching { it.release() } }
        record.release()
    }

    fun stop() {
        running = false
    }

    companion object {
        /** The parts of [MicSettings] baked into the recorder at creation; the rest applies live in the engine. */
        fun recorderSettings(s: MicSettings) = listOf(s.mode, s.systemAgc, s.systemNoiseSuppression, s.systemEchoCancellation)

        private fun source(mode: String): Int = when (mode) {
            "default" -> MediaRecorder.AudioSource.DEFAULT
            "raw" -> MediaRecorder.AudioSource.UNPROCESSED
            "voicePerformance" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) MediaRecorder.AudioSource.VOICE_PERFORMANCE else MediaRecorder.AudioSource.MIC
            "voiceRecognition" -> MediaRecorder.AudioSource.VOICE_RECOGNITION
            "camcorder" -> MediaRecorder.AudioSource.CAMCORDER
            "mic" -> MediaRecorder.AudioSource.MIC
            else -> MediaRecorder.AudioSource.VOICE_COMMUNICATION
        }

        @SuppressLint("MissingPermission") // The engine refuses mic routes without RECORD_AUDIO.
        fun start(settings: MicSettings): MicCapture? {
            val minBuffer = AudioRecord.getMinBufferSize(48_000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
            val record = runCatching {
                AudioRecord(source(settings.mode), 48_000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, maxOf(minBuffer, 960 * 4))
            }.getOrNull()?.takeIf { it.state == AudioRecord.STATE_INITIALIZED } ?: return null

            val session = record.audioSessionId
            val effects = buildList {
                if (AcousticEchoCanceler.isAvailable()) {
                    AcousticEchoCanceler.create(session)?.also {
                        it.enabled = settings.systemEchoCancellation
                        add(it)
                    }
                }
                if (NoiseSuppressor.isAvailable()) {
                    NoiseSuppressor.create(session)?.also {
                        it.enabled = settings.systemNoiseSuppression
                        add(it)
                    }
                }
                if (AutomaticGainControl.isAvailable()) {
                    AutomaticGainControl.create(session)?.also {
                        it.enabled = settings.systemAgc
                        add(it)
                    }
                }
            }
            return MicCapture(record, effects)
        }
    }
}
