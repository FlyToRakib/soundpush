package net.soundpush.ui.components

import androidx.annotation.StringRes
import net.soundpush.ui.R

/**
 * Explicit engine-value → string mappings. (Looking strings up by name at runtime is
 * fragile: resource shrinking in release builds can remove "unused" strings.)
 */
object Labels {
    @StringRes
    fun status(connection: String): Int = when (connection) {
        "connected" -> R.string.status_connected
        "connecting" -> R.string.status_connecting
        "reconnecting" -> R.string.status_reconnecting
        "waitingForDevice" -> R.string.status_waitingForDevice
        "pairingRequired" -> R.string.status_pairingRequired
        "incompatible" -> R.string.status_incompatible
        else -> R.string.status_disconnected
    }

    @StringRes
    fun quality(quality: String): Int? = when (quality) {
        "excellent" -> R.string.quality_excellent
        "good" -> R.string.quality_good
        "poor" -> R.string.quality_poor
        else -> null
    }

    @StringRes
    fun routeTitle(kind: String): Int = when (kind) {
        "receiveSystemAudio" -> R.string.route_receiveSystemAudio
        "receiveAppAudio" -> R.string.route_receiveAppAudio
        "sendMicToVirtualMic" -> R.string.route_sendMicToVirtualMic
        "sendMicToSpeaker" -> R.string.route_sendMicToSpeaker
        "sendAppAudio" -> R.string.route_sendAppAudio
        "sendSystemAudio" -> R.string.route_sendSystemAudio
        "receiveMicToVirtualMic" -> R.string.route_receiveMicToVirtualMic
        else -> R.string.route_receiveMicToSpeaker
    }

    /** Title for a request coming from another device. `kind` is from this phone's point of view. */
    @StringRes
    fun requestTitle(kind: String): Int = when {
        kind.startsWith("sendMic") -> R.string.request_mic
        kind.startsWith("send") -> R.string.request_listen
        else -> R.string.request_play
    }

    @StringRes
    fun error(key: String): Int = when (key) {
        "error.device.notFound" -> R.string.error_device_not_found
        "error.network.unreachable" -> R.string.error_network_unreachable
        "error.network.blocked" -> R.string.error_network_blocked
        "error.security.notPaired" -> R.string.error_not_paired
        "error.security.pairingRejected" -> R.string.error_pairing_rejected
        "error.security.pairingExpired" -> R.string.error_pairing_expired
        "error.security.revoked" -> R.string.error_revoked
        "error.compat.version" -> R.string.error_version
        "error.permission.peerDenied" -> R.string.error_peer_denied
        "error.permission.mic" -> R.string.error_mic_permission
        "error.audio.device" -> R.string.error_audio_device
        "error.audio.loopbackUnsupported" -> R.string.error_loopback
        "error.audio.virtualMicMissing" -> R.string.error_virtual_mic
        "error.input.invalid" -> R.string.error_invalid
        "error.engine.starting" -> R.string.error_starting
        else -> R.string.error_generic
    }
}
