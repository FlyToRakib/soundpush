// Mirrors sp-engine's serialized EngineState (serde camelCase).

export type Theme = "system" | "light" | "dark";
export type Visibility = "everyone" | "trustedOnly" | "hidden";
export type LatencyMode = "lowLatency" | "balanced" | "stable" | "custom";
export type QualityMode = "auto" | "opus" | "lossless";
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
}

export interface CaptureSettings {
  systemDevice: string | null;
  muteLocalSpeakers: boolean;
}

export interface DesktopSettings {
  launchAtLogin: boolean;
  startMinimized: boolean;
  closeToTray: boolean;
  preventSleepWhileStreaming: boolean;
  muteHotkey: string | null;
  pushToTalkHotkey: string | null;
  virtualMicDevice: string | null;
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
  resumeRoutesOnStart: boolean;
  savedRoutes: { peerId: string; kind: RouteKind }[];
  dismissedTips: string[];
  audioCues: boolean;
}

export interface LocalDevice {
  deviceId: string;
  displayCode: string;
  name: string;
  platform: string;
  port: number;
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
}
