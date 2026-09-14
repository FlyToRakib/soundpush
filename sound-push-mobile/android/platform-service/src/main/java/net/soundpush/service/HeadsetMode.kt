package net.soundpush.service

import net.soundpush.engine.MicSettings
import net.soundpush.engine.RouteView

/**
 * Headset task (plan §15.7): while this phone plays a device's audio and sends the microphone to
 * the same device, the recorder uses the voice-call preset with echo cancellation, so the sound
 * coming out of the phone is not sent back. The user's own microphone settings are never changed;
 * the recorder reopens with them as soon as the headset task ends.
 */
internal object HeadsetMode {
    const val PRESET = "voiceCommunication"

    /** Sending this phone's microphone to a device it is also playing audio from. */
    fun active(routes: List<RouteView>): Boolean {
        val live = routes.filter { it.status != "stopped" }
        return live.any { mic -> mic.isSending && mic.isMic && live.any { !it.isSending && it.peerId == mic.peerId } }
    }

    /** The settings the recorder opens with. */
    fun recorderSettings(user: MicSettings, headset: Boolean): MicSettings =
        if (headset) user.copy(mode = PRESET, systemEchoCancellation = true) else user
}
