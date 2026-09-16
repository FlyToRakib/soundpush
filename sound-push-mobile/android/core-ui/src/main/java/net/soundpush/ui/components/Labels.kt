package net.soundpush.ui.components

import androidx.annotation.StringRes
import net.soundpush.engine.DeviceStatus
import net.soundpush.ui.R

/**
 * Explicit engine-value → string mappings. (Looking strings up by name at runtime is
 * fragile: resource shrinking in release builds can remove "unused" strings.)
 */
object Labels {
    /** Where media plays now (notification output action, connection details). */
    @StringRes
    fun output(output: DeviceStatus.Output): Int = when (output) {
        DeviceStatus.Output.Speaker -> R.string.output_speaker
        DeviceStatus.Output.Wired -> R.string.output_wired
        DeviceStatus.Output.Bluetooth -> R.string.output_bluetooth
        DeviceStatus.Output.Usb -> R.string.output_usb
        DeviceStatus.Output.Other -> R.string.output_other
    }

    @StringRes
    fun status(connection: String): Int = when (connection) {
        "connected" -> R.string.status_connected
        "degraded" -> R.string.status_degraded
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

    /** Engine notices (`EngineState.notices`); their `args` fill the placeholders. */
    @StringRes
    fun notice(key: String): Int = when (key) {
        "notice.pairingSuccess" -> R.string.notice_pairing_success
        "notice.peerForgotUs" -> R.string.notice_peer_forgot_us
        "notice.identityReset" -> R.string.notice_identity_reset
        "notice.trustStoreRecovered" -> R.string.notice_trust_store_recovered
        "notice.settingsRecovered" -> R.string.notice_settings_recovered
        "notice.muteSpeakersUnsupported" -> R.string.notice_mute_speakers_unsupported
        "notice.crashReport" -> R.string.notice_crash_report
        "notice.unstableConnection" -> R.string.notice_unstable_connection
        "notice.pairingRateLimited" -> R.string.notice_pairing_rate_limited
        "notice.qualityFallback" -> R.string.notice_quality_fallback
        "notice.qualityRestored" -> R.string.notice_quality_restored
        else -> R.string.notice_generic
    }

    /** Network test tips (`NetworkReport.recommendation.tips`). */
    @StringRes
    fun networkTip(key: String): Int? = when (key) {
        "nettest.tip.good" -> R.string.nettest_tip_good
        "nettest.tip.losslessOk" -> R.string.nettest_tip_losslessOk
        "nettest.tip.loss" -> R.string.nettest_tip_loss
        "nettest.tip.jitter" -> R.string.nettest_tip_jitter
        "nettest.tip.latency" -> R.string.nettest_tip_latency
        "nettest.tip.bandwidth" -> R.string.nettest_tip_bandwidth
        else -> null
    }

    @StringRes
    fun error(key: String): Int = when (key) {
        "error.device.notFound" -> R.string.error_device_not_found
        "error.network.unreachable" -> R.string.error_network_unreachable
        "error.network.blocked" -> R.string.error_network_blocked
        "error.security.notPaired" -> R.string.error_not_paired
        "error.security.pairingRejected" -> R.string.error_pairing_rejected
        "error.security.pairingExpired" -> R.string.error_pairing_expired
        "error.security.pairingRateLimited" -> R.string.error_pairing_rate_limited
        "error.security.revoked" -> R.string.error_revoked
        "error.compat.version" -> R.string.error_version
        "error.permission.peerDenied" -> R.string.error_peer_denied
        "error.permission.mic" -> R.string.error_mic_permission
        "error.audio.device" -> R.string.error_audio_device
        "error.audio.loopbackUnsupported" -> R.string.error_loopback
        "error.audio.virtualMicMissing" -> R.string.error_virtual_mic
        "error.route.tooManyReceivers" -> R.string.error_too_many_receivers
        "error.input.invalid" -> R.string.error_invalid
        "error.engine.starting" -> R.string.error_starting
        else -> R.string.error_generic
    }
}
