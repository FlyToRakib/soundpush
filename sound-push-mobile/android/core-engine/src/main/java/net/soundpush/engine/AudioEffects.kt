package net.soundpush.engine

import android.media.audiofx.AcousticEchoCanceler
import android.media.audiofx.AutomaticGainControl
import android.media.audiofx.NoiseSuppressor

/** Which platform microphone effects this device offers (shown greyed out when missing). */
object AudioEffects {
    val echoCancellation: Boolean by lazy { runCatching { AcousticEchoCanceler.isAvailable() }.getOrDefault(false) }
    val noiseSuppression: Boolean by lazy { runCatching { NoiseSuppressor.isAvailable() }.getOrDefault(false) }
    val automaticGain: Boolean by lazy { runCatching { AutomaticGainControl.isAvailable() }.getOrDefault(false) }
}
