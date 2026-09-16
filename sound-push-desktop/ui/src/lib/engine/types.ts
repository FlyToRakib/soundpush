// Mirrors sp-engine's serialized EngineState (serde camelCase).

export type Theme = "system" | "light" | "dark";
export type Visibility = "everyone" | "trustedOnly" | "hidden";
export type LatencyMode = "lowLatency" | "balanced" | "stable" | "custom";
export type QualityMode = "auto" | "opus" | "lossless";
/** Pinned transport (Advanced settings). "auto" races QUIC against the USB link. */
export type TransportPin = "auto" | "quic" | "tcp" | "usb";
export type AudioFocusMode = "pause" | "duck" | "mix" | "mixDuringCalls";
export type MicMode =
  | "default"
  | "voiceCommunication"
  | "raw"
  | "voicePerformance"
  | "voiceRecognition"
  | "camcorder"
  | "mic";

export type RouteKind =
  | "sendSystemAudio"
  | "sendAppAudio"
  | "sendMicToVirtualMic"
  | "sendMicToSpeaker"
  | "receiveSystemAudio"
  | "receiveAppAudio"
  | "receiveMicToVirtualMic"
  | "receiveMicToSpeaker";

export type RouteStatus = "requesting" | "waitingForApproval" | "starting" | "active" | "paused" | "stopped";
export type ConnectionStatus =
  | "disconnected"
  | "connecting"
  | "pairingRequired"
  | "connected"
  /** Connected, but loss or jitter is high; streams keep running. */
  | "degraded"
  | "reconnecting"
  | "waitingForDevice"
  | "incompatible";
export type LinkQuality = "unknown" | "excellent" | "good" | "poor";
export type Policy = "allow" | "ask" | "deny";
export type PermissionKind = "receiveMyAudio" | "useMyMicrophone" | "sendAudioToMe" | "controlMe";
export type Severity = "info" | "warning" | "error";
export type FixAction =
  | "openMicPermissionSettings"
  | "installVirtualMic"
  | "openFirewallFix"
  | "switchToUsb"
  | "updatePeerApp"
  | "retryConnection"
  | "pairAgain"
  | "openBatterySettings"
  | "chooseAnotherDevice";

export interface Permissions {
  receive_my_audio: Policy;
  use_my_microphone: Policy;
  send_audio_to_me: Policy;
  control_me: Policy;
}

export interface StreamSettings {
  latency: LatencyMode;
  customMinMs: number;
  customMaxMs: number;
  quality: QualityMode;
  opusBitrate: number;
  redundancy: boolean;
}

export interface OutputSettings {
  device: string | null;
  volume: number;
  balance: number;
  mono: boolean;
  avOffsetMs: number;
  compatibilityOutput: boolean;
  outputEffects: boolean;
  pauseOnHeadsetDisconnect: boolean;
  audioFocus: AudioFocusMode;
}

export interface MicSettings {
  device: string | null;
  gainDb: number;
  noiseSuppression: boolean;
  mode: MicMode;
  systemAgc: boolean;
  systemNoiseSuppression: boolean;
  systemEchoCancellation: boolean;
  monitor: boolean;
  /** 80 Hz high-pass filter before noise suppression. */
  highPass: boolean;
}

export interface CaptureSettings {
  systemDevice: string | null;
  muteLocalSpeakers: boolean;
  /** Send only this app (executable name); null sends everything. Windows 10 2004+. */
  app: string | null;
  /** Send everything except `app`. */
  excludeApp: boolean;
}

export interface DesktopSettings {
  launchAtLogin: boolean;
  startMinimized: boolean;
  closeToTray: boolean;
  preventSleepWhileStreaming: boolean;
  muteHotkey: string | null;
  pushToTalkHotkey: string | null;
  virtualMicDevice: string | null;
  /** Start the phone microphone when another app opens the virtual microphone. */
  autoStartMic: boolean;
  lastMicPeer: string | null;
}

/** Per-device overrides of StreamSettings; absent fields follow the global setting. */
export interface DeviceProfile {
  latency?: LatencyMode;
  customMinMs?: number;
  customMaxMs?: number;
  quality?: QualityMode;
  opusBitrate?: number;
  redundancy?: boolean;
}

export interface SavedRoute {
  peerId: string;
  kind: RouteKind;
  /** True for "Keep running after restart"; false when saved by "Resume streams after restart". */
  keep: boolean;
}

export interface Recommendation {
  latency: LatencyMode;
  quality: QualityMode;
  opusBitrate: number;
  redundancy: boolean;
  /** i18n keys, most important first. */
  tips: string[];
}

export interface NetworkReport {
  transport: "quic" | "tcp";
  rttMs: number;
  rttP95Ms: number;
  jitterMs: number;
  lossPct: number;
  achievableKbps: number;
  maxDatagramBytes: number;
  probesSent: number;
  probesReceived: number;
  durationMs: number;
  recommendation: Recommendation;
}

export type NetworkTestStatus = "running" | "done" | "failed" | "cancelled";

export interface NetworkTestView {
  peerId: string;
  status: NetworkTestStatus;
  progress: number;
  report: NetworkReport | null;
  error: ErrorView | null;
  startedUnix: number;
}

export interface Settings {
  version: number;
  deviceName: string;
  theme: Theme;
  language: string;
  visibility: Visibility;
  stream: StreamSettings;
  output: OutputSettings;
  mic: MicSettings;
  capture: CaptureSettings;
  desktop: DesktopSettings;
  mobile: { stayAvailable: boolean; remindAfterRestart: boolean };
  autoConnectTrusted: boolean;
  /** Transport pinned in Advanced settings; "auto" lets the engine choose. */
  transport: TransportPin;
  /** Most devices that may receive the same source at once (1–16, default 8). */
  maxReceivers: number;
  resumeRoutesOnStart: boolean;
  savedRoutes: SavedRoute[];
  dismissedTips: string[];
  audioCues: boolean;
  /** Keyed by device id. */
  deviceProfiles: Record<string, DeviceProfile>;
  checkForUpdates: boolean;
  updateChannel: UpdateChannel;
  /** Detailed logging (plan §28.1). Present only when the engine supports it; the engine switches it
   *  off again after 24 hours. */
  debugLogging?: boolean;
  /** When debug logging switches itself off (unix seconds, 0 while off). Set by the engine. */
  debugLoggingUntilUnix?: number;
}

/** Desktop update channel (docs/release-signing.md). */
export type UpdateChannel = "stable" | "beta";

export interface LocalDevice {
  deviceId: string;
  displayCode: string;
  name: string;
  platform: string;
  port: number;
  /** Loopback TCP port for USB via adb reverse (0 = not listening). */
  tcpPort: number;
  addresses: string[];
  appVersion: string;
}

export interface PeerView {
  deviceId: string;
  name: string;
  platform: string;
  trusted: boolean;
  online: boolean;
  connection: ConnectionStatus;
  quality: LinkQuality;
  rttMs: number;
  addresses: string[];
  permissions: Permissions | null;
  autoConnect: boolean;
  blocked: boolean;
  lastSeenUnix: number;
  canSendSystemAudio: boolean;
  canSendAppAudio: boolean;
  canSendMic: boolean;
  canPlay: boolean;
  hasVirtualMic: boolean;
  /** "quic" or "tcp" (USB) while connected, "" otherwise. */
  transport: "" | "quic" | "tcp";
  /** Other end of the connection ("192.168.1.20:47650", "[fe80::1%3]:47650"), "" while disconnected. */
  remoteAddress: string;
  /** This computer asked the device to mute its speakers. */
  speakersMuted: boolean;
}

export interface RouteStats {
  codec: string;
  bitrateKbps: number;
  latencyMs: number;
  bufferMs: number;
  jitterMs: number;
  lossPct: number;
  underruns: number;
  driftPpm: number;
  levelDb: number;
  /** Parts of latencyMs; the jitter buffer is bufferMs. */
  captureMs: number;
  encodeMs: number;
  networkMs: number;
  outputMs: number;
}

export interface RouteView {
  routeId: string;
  peerId: string;
  peerName: string;
  kind: RouteKind;
  status: RouteStatus;
  startedUnix: number;
  elapsedSecs: number;
  volume: number;
  muted: boolean;
  stats: RouteStats;
  keepRunning: boolean;
}

export interface PairingPrompt {
  peerId: string;
  peerName: string;
  platform: string;
  code: string;
  peerConfirmed: boolean;
}

export interface RouteRequestPrompt {
  requestId: number;
  peerId: string;
  peerName: string;
  kind: RouteKind;
  expiresUnix: number;
}

export interface ErrorView {
  /** Stable support code, e.g. "SP-NET-004" (docs/error-codes.md). */
  code: string;
  key: string;
  message: string;
  severity: Severity;
  retryable: boolean;
  fix: FixAction | null;
}

export interface NoticeView {
  id: number;
  key: string;
  args: string[];
  severity: Severity;
  error: ErrorView | null;
  createdUnix: number;
}

/** Local security log (sp-engine audit.rs). */
export type AuditKind =
  | "pairingAttempt"
  | "pairingSucceeded"
  | "pairingRejected"
  | "pairingRateLimited"
  | "deviceForgotten"
  | "deviceBlocked"
  | "deviceUnblocked"
  | "permissionChanged"
  | "routeApproved"
  | "routeDenied"
  | "routeStarted"
  | "routeStopped"
  | "connectionRefused"
  | "logCleared";

export interface AuditEntry {
  timeUnix: number;
  kind: AuditKind;
  peerName: string;
  peerCode: string;
  route?: RouteKind;
  /** Kind-specific: remote address, `permission=policy`, rejection or stop reason. */
  detail: string;
}

export interface AudioDeviceView {
  id: string;
  name: string;
  isInput: boolean;
  isDefault: boolean;
  /** Playback side of a virtual cable that other apps can use as a microphone. */
  virtualCable: boolean;
}

export interface EngineState {
  revision: number;
  local: LocalDevice;
  peers: PeerView[];
  routes: RouteView[];
  pairing: { qrUri: string | null; qrExpiresUnix: number; prompts: PairingPrompt[] };
  requests: RouteRequestPrompt[];
  notices: NoticeView[];
  settings: Settings;
  capabilities: {
    systemAudio: boolean;
    appAudio: boolean;
    microphone: boolean;
    speaker: boolean;
    virtualMic: boolean;
    /** Device the phone microphone is fed into. */
    virtualMicDevice: string | null;
    /** What other apps select as their microphone. */
    virtualMicInput: string | null;
  };
  audioDevices: AudioDeviceView[];
  micLevelDb: number;
  networkTests: NetworkTestView[];
  /** Microphone mute (tray, shortcuts, push-to-talk). */
  micMuted: boolean;
  streaming: StreamingLoad;
}

/**
 * Multi-device streaming: how many devices receive this computer's audio, the limit, and a rough
 * estimate of what it costs, so the cost of one more can be shown before it is added.
 */
export interface StreamingLoad {
  receivers: number;
  maxReceivers: number;
  /** Adding a receiver beyond this is where we warn. */
  safeReceivers: number;
  kbps: number;
  cpuPct: number;
  perReceiverKbps: number;
  perReceiverCpuPct: number;
}

// ---- desktop platform (src-tauri: network.rs, system.rs, hotkeys.rs)

export interface NetworkStatus {
  supported: boolean;
  firewallEnabled: boolean;
  /** Other devices can't connect to this computer. */
  blocked: boolean;
  publicNetwork: boolean;
  publicNetworkName: string | null;
  allowedOnPublic: boolean;
}

export type PermissionState = "granted" | "denied" | "notDetermined" | "restricted" | "unknown";

export interface SystemStatus {
  microphone: PermissionState;
  systemAudio: PermissionState;
  mediaFeaturePackMissing: boolean;
  windowsEdition: string | null;
  autostartDisabledByOs: boolean;
  bluetoothOutputs: string[];
  defaultOutput: string | null;
  appCapture: boolean;
}

export type HotkeyKind = "mute" | "pushToTalk";
export type HotkeyError = "invalid" | "duplicate" | "unavailable";
export interface HotkeyStatus {
  mute: HotkeyError | null;
  pushToTalk: HotkeyError | null;
}

export interface AudioApps {
  supported: boolean;
  apps: { process: string; active: boolean }[];
}

/** USB tethering to a phone (src-tauri tethering.rs). */
export interface TetheringStatus {
  active: boolean;
  /** This computer's internet goes through the phone's mobile data. */
  internetViaPhone: boolean;
  /** Connected devices reached through the tethering network. */
  peers: string[];
}

/** What a diagnostics export contains (src-tauri commands.rs `preview_diagnostics`). */
export interface DiagnosticsSection {
  id: "system" | "state" | "crashes" | "log";
  count: number;
  redacted: ("addresses" | "deviceIds" | "pairingCode")[];
}

export interface DiagnosticsPreview {
  sections: DiagnosticsSection[];
  /** Exactly the text an export writes. */
  text: string;
}

export type SettingsTopic = "microphone" | "systemAudio" | "network" | "optionalFeatures" | "sound" | "startup";
