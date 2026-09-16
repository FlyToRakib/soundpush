package net.soundpush.engine

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/** Mirrors sp-engine's serialized state. Unknown fields are ignored so the engine can evolve. */
val EngineJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = true
}

@Serializable
data class EngineState(
    val revision: Long = 0,
    val local: LocalDevice = LocalDevice(),
    val peers: List<PeerView> = emptyList(),
    val routes: List<RouteView> = emptyList(),
    val pairing: PairingView = PairingView(),
    val requests: List<RouteRequestPrompt> = emptyList(),
    val notices: List<NoticeView> = emptyList(),
    val settings: Settings = Settings(),
    val capabilities: Capabilities = Capabilities(),
    val micLevelDb: Float = -120f,
    val networkTests: List<NetworkTestView> = emptyList(),
    val streaming: StreamingLoad = StreamingLoad(),
) {
    val trustedPeers get() = peers.filter { it.trusted }
    val connectedPeers get() = trustedPeers.filter { it.isConnected }
    val nearbyUntrusted get() = peers.filter { !it.trusted && it.online }
}

@Serializable
data class LocalDevice(
    val deviceId: String = "",
    val displayCode: String = "",
    val name: String = "",
    val platform: String = "android",
    val port: Int = 0,
    val addresses: List<String> = emptyList(),
    val appVersion: String = "",
)

@Serializable
data class Capabilities(
    val systemAudio: Boolean = false,
    val appAudio: Boolean = false,
    val microphone: Boolean = true,
    val speaker: Boolean = true,
    val virtualMic: Boolean = false,
)

/**
 * Multi-device streaming (plan §19.2): devices receiving this one's audio, the limit, and a rough
 * estimate of the bandwidth and CPU it costs, so the cost of one more can be shown beforehand.
 */
@Serializable
data class StreamingLoad(
    val receivers: Int = 0,
    val maxReceivers: Int = 8,
    /** Adding a receiver beyond this is where the app warns. */
    val safeReceivers: Int = 8,
    val kbps: Int = 0,
    val cpuPct: Int = 0,
    val perReceiverKbps: Int = 0,
    val perReceiverCpuPct: Int = 0,
)

@Serializable
data class Permissions(
    val receive_my_audio: String = "allow",
    val use_my_microphone: String = "ask",
    val send_audio_to_me: String = "allow",
    val control_me: String = "allow",
)

@Serializable
data class PeerView(
    val deviceId: String,
    val name: String,
    val platform: String = "",
    val trusted: Boolean = false,
    val online: Boolean = false,
    val connection: String = "disconnected",
    val quality: String = "unknown",
    val rttMs: Double = 0.0,
    val addresses: List<String> = emptyList(),
    val permissions: Permissions? = null,
    val autoConnect: Boolean = false,
    val blocked: Boolean = false,
    val canSendSystemAudio: Boolean = false,
    val canSendAppAudio: Boolean = false,
    val canSendMic: Boolean = false,
    val canPlay: Boolean = false,
    val hasVirtualMic: Boolean = false,
    /** "quic" or "tcp" (USB) while connected. */
    val transport: String = "",
    /** This phone muted the device's own speakers ("Mute computer speakers") in this session. */
    val speakersMuted: Boolean = false,
) {
    /** A session exists: "connected", or "degraded" (connected with high loss or jitter). */
    val isConnected get() = connection == "connected" || connection == "degraded"
}

@Serializable
data class Recommendation(
    val latency: String = "balanced",
    val quality: String = "auto",
    val opusBitrate: Int = 128_000,
    val redundancy: Boolean = false,
    val tips: List<String> = emptyList(),
)

@Serializable
data class NetworkReport(
    val transport: String = "",
    val rttMs: Double = 0.0,
    val rttP95Ms: Double = 0.0,
    val jitterMs: Double = 0.0,
    val lossPct: Double = 0.0,
    val achievableKbps: Int = 0,
    val maxDatagramBytes: Int = 0,
    val probesSent: Int = 0,
    val probesReceived: Int = 0,
    val durationMs: Int = 0,
    val recommendation: Recommendation = Recommendation(),
)

@Serializable
data class NetworkTestView(
    val peerId: String,
    /** running | done | failed | cancelled */
    val status: String = "running",
    val progress: Float = 0f,
    val report: NetworkReport? = null,
    val error: ErrorView? = null,
)

/** Per-device overrides; null follows the global setting. */
@Serializable
data class DeviceProfile(
    val latency: String? = null,
    val customMinMs: Int? = null,
    val customMaxMs: Int? = null,
    val quality: String? = null,
    val opusBitrate: Int? = null,
    val redundancy: Boolean? = null,
)

@Serializable
data class RouteStats(
    val codec: String = "",
    val bitrateKbps: Int = 0,
    val latencyMs: Double = 0.0,
    val bufferMs: Double = 0.0,
    val jitterMs: Double = 0.0,
    val lossPct: Double = 0.0,
    val underruns: Long = 0,
    val driftPpm: Int = 0,
    val levelDb: Float = -120f,
)

@Serializable
data class RouteView(
    val routeId: String,
    val peerId: String,
    val peerName: String,
    val kind: String,
    val status: String,
    val elapsedSecs: Long = 0,
    val volume: Float = 1f,
    val muted: Boolean = false,
    val stats: RouteStats = RouteStats(),
    val keepRunning: Boolean = false,
) {
    val isMic get() = kind.contains("Mic")
    val isSending get() = kind.startsWith("send")
}

/** One entry of the local security log (sp-engine audit.rs); [kind] and [detail] as documented there. */
@Serializable
data class AuditEntry(
    val timeUnix: Long = 0,
    val kind: String = "",
    val peerName: String = "",
    val peerCode: String = "",
    val route: String? = null,
    val detail: String = "",
)

@Serializable
data class PairingPrompt(
    val peerId: String,
    val peerName: String,
    val platform: String = "",
    val code: String,
    val peerConfirmed: Boolean = false,
)

@Serializable
data class PairingView(
    val qrUri: String? = null,
    val qrExpiresUnix: Long = 0,
    val prompts: List<PairingPrompt> = emptyList(),
)

@Serializable
data class RouteRequestPrompt(
    val requestId: Long,
    val peerId: String,
    val peerName: String,
    val kind: String,
    val expiresUnix: Long = 0,
)

@Serializable
data class ErrorView(
    val key: String,
    /** Stable support code, e.g. `SP-NET-004` (docs/error-codes.md). */
    val code: String = "",
    val message: String = "",
    val severity: String = "error",
    val retryable: Boolean = false,
    val fix: String? = null,
)

@Serializable
data class NoticeView(
    val id: Long,
    val key: String,
    val args: List<String> = emptyList(),
    val severity: String = "info",
    val error: ErrorView? = null,
)

@Serializable
data class StreamSettings(
    val latency: String = "balanced",
    val customMinMs: Int = 30,
    val customMaxMs: Int = 120,
    val quality: String = "auto",
    val opusBitrate: Int = 128_000,
    val redundancy: Boolean = false,
)

@Serializable
data class OutputSettings(
    val device: String? = null,
    val volume: Float = 1f,
    val balance: Float = 0f,
    val mono: Boolean = false,
    val avOffsetMs: Int = 0,
    val compatibilityOutput: Boolean = false,
    val outputEffects: Boolean = false,
    val pauseOnHeadsetDisconnect: Boolean = true,
    val audioFocus: String = "pause",
)

@Serializable
data class MicSettings(
    val device: String? = null,
    val gainDb: Float = 0f,
    val noiseSuppression: Boolean = false,
    val mode: String = "voiceCommunication",
    val systemAgc: Boolean = false,
    val systemNoiseSuppression: Boolean = true,
    val systemEchoCancellation: Boolean = true,
    val monitor: Boolean = false,
    val highPass: Boolean = true,
)

@Serializable
data class CaptureSettings(val systemDevice: String? = null, val muteLocalSpeakers: Boolean = false)

@Serializable
data class DesktopSettings(
    val launchAtLogin: Boolean = true,
    val startMinimized: Boolean = true,
    val closeToTray: Boolean = true,
    val preventSleepWhileStreaming: Boolean = false,
    val muteHotkey: String? = null,
    val pushToTalkHotkey: String? = null,
    val virtualMicDevice: String? = null,
)

@Serializable
data class MobileSettings(val stayAvailable: Boolean = false, val remindAfterRestart: Boolean = true)

@Serializable
data class SavedRoute(val peerId: String, val kind: String, val keep: Boolean = true)

@Serializable
data class Settings(
    val version: Int = 3,
    val deviceName: String = "",
    val theme: String = "system",
    val language: String = "system",
    val visibility: String = "trustedOnly",
    val stream: StreamSettings = StreamSettings(),
    val output: OutputSettings = OutputSettings(),
    val mic: MicSettings = MicSettings(),
    val capture: CaptureSettings = CaptureSettings(),
    val desktop: DesktopSettings = DesktopSettings(),
    val mobile: MobileSettings = MobileSettings(),
    val autoConnectTrusted: Boolean = true,
    /** Pinned transport: auto | quic | tcp | usb (plan §16.1). */
    val transport: String = "auto",
    /** Most devices that may receive the same source at once, 1–16 (plan §19.2). */
    val maxReceivers: Int = 8,
    val resumeRoutesOnStart: Boolean = false,
    val savedRoutes: List<SavedRoute> = emptyList(),
    val dismissedTips: List<String> = emptyList(),
    val audioCues: Boolean = false,
    /** Keyed by device id; kept here so settings written from this app don't drop profiles. */
    val deviceProfiles: Map<String, DeviceProfile> = emptyMap(),
    val checkForUpdates: Boolean = true,
    /** Kept so settings written from this app don't switch debug logging off. */
    val debugLogging: Boolean = false,
    val debugLoggingUntilUnix: Long = 0,
)
