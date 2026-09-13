// In-browser mock engine so the UI can be previewed and tested without Tauri.
import type { EngineState, RouteKind, Settings } from "./types";

const settings: Settings = {
  version: 1,
  deviceName: "My PC",
  theme: "system",
  language: "system",
  visibility: "trustedOnly",
  stream: { latency: "balanced", customMinMs: 30, customMaxMs: 120, quality: "auto", opusBitrate: 128000, redundancy: false },
  output: {
    device: null,
    volume: 1,
    balance: 0,
    mono: false,
    avOffsetMs: 0,
    compatibilityOutput: false,
    outputEffects: false,
    pauseOnHeadsetDisconnect: true,
    audioFocus: "pause",
  },
  mic: {
    device: null,
    gainDb: 0,
    noiseSuppression: false,
    mode: "voiceCommunication",
    systemAgc: false,
    systemNoiseSuppression: true,
    systemEchoCancellation: true,
    monitor: false,
  },
  capture: { systemDevice: null, muteLocalSpeakers: false },
  desktop: {
    launchAtLogin: true,
    startMinimized: true,
    closeToTray: true,
    preventSleepWhileStreaming: false,
    muteHotkey: null,
    pushToTalkHotkey: null,
    virtualMicDevice: null,
  },
  mobile: { stayAvailable: false, remindAfterRestart: true },
  autoConnectTrusted: true,
  resumeRoutesOnStart: false,
  savedRoutes: [],
  dismissedTips: [],
  audioCues: false,
};

let state: EngineState = {
  revision: 1,
  local: {
    deviceId: "00".repeat(16),
    displayCode: "SP-0000-0000-0000",
    name: settings.deviceName,
    platform: "windows",
    port: 47650,
    addresses: ["192.168.1.10:47650"],
    appVersion: "0.1.0",
  },
  peers: [
    {
      deviceId: "11".repeat(16),
      name: "Redmi Note 9 Pro",
      platform: "android",
      trusted: true,
      online: true,
      connection: "connected",
      quality: "excellent",
      rttMs: 4,
      addresses: [],
      permissions: { receive_my_audio: "allow", use_my_microphone: "ask", send_audio_to_me: "allow", control_me: "allow" },
      autoConnect: true,
      blocked: false,
      lastSeenUnix: 0,
      canSendSystemAudio: false,
      canSendAppAudio: true,
      canSendMic: true,
      canPlay: true,
      hasVirtualMic: false,
    },
  ],
  routes: [],
  pairing: { qrUri: null, qrExpiresUnix: 0, prompts: [] },
  requests: [],
  notices: [],
  settings,
  capabilities: {
    systemAudio: true,
    appAudio: false,
    microphone: true,
    speaker: true,
    virtualMic: false,
    virtualMicDevice: null,
    virtualMicInput: null,
  },
  audioDevices: [
    { id: "Speakers", name: "Speakers", isInput: false, isDefault: true, virtualCable: false },
    { id: "Microphone", name: "Microphone", isInput: true, isDefault: true, virtualCable: false },
  ],
  micLevelDb: -120,
};

const listeners = new Set<(s: EngineState) => void>();
function emit(patch: Partial<EngineState>) {
  state = { ...state, ...patch, revision: state.revision + 1 };
  listeners.forEach((l) => l(state));
}

let mockDriverInstalled = false;

export const mockEngine = {
  subscribe(handler: (s: EngineState) => void) {
    listeners.add(handler);
    handler(state);
    return () => listeners.delete(handler);
  },
  async invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    switch (cmd) {
      case "get_state":
        return state as T;
      case "start_pairing": {
        // Same length and character set as a real compact pairing code (SP1: + base32).
        const uri = "SP1:AGKZ4TQ7M3XDWVHNRL5PJYCE6BFUA3SQ7KZMXDNWVHR4TLPJYCE6BFUAXOTWEAAAQCAKBAEAQCAIBQ";
        emit({ pairing: { ...state.pairing, qrUri: uri, qrExpiresUnix: Date.now() / 1000 + 300 } });
        return uri as T;
      }
      case "stop_pairing":
        emit({ pairing: { ...state.pairing, qrUri: null } });
        return undefined as T;
      case "update_settings": {
        const next = args?.settings as Settings;
        emit({ settings: next, local: { ...state.local, name: next.deviceName } });
        return next as T;
      }
      case "start_route": {
        const kind = args?.kind as RouteKind;
        const routeId = `mock-${state.routes.length + 1}`;
        emit({
          routes: [
            ...state.routes,
            {
              routeId,
              peerId: String(args?.deviceId),
              peerName: "Phone",
              kind,
              status: "active",
              startedUnix: Date.now() / 1000,
              elapsedSecs: 0,
              volume: 1,
              muted: false,
              keepRunning: false,
              stats: { codec: "Opus", bitrateKbps: 128, latencyMs: 48, bufferMs: 30, jitterMs: 2, lossPct: 0, underruns: 0, driftPpm: 12, levelDb: -18 },
            },
          ],
        });
        return routeId as T;
      }
      case "stop_route":
        emit({ routes: state.routes.filter((r) => r.routeId !== args?.routeId) });
        return undefined as T;
      case "forget_device":
        emit({ peers: state.peers.filter((p) => p.deviceId !== args?.deviceId) });
        return undefined as T;
      case "dismiss_notice":
        emit({ notices: state.notices.filter((n) => n.id !== args?.id) });
        return undefined as T;
      case "export_diagnostics":
        return "/tmp/soundpush-diagnostics.zip" as T;
      case "virtual_mic_status":
        return { supported: true, installed: mockDriverInstalled } as T;
      case "install_virtual_mic":
      case "uninstall_virtual_mic": {
        mockDriverInstalled = cmd === "install_virtual_mic";
        const input = mockDriverInstalled ? "SoundPush Microphone" : null;
        emit({
          capabilities: { ...state.capabilities, virtualMic: mockDriverInstalled, virtualMicDevice: input, virtualMicInput: input },
          audioDevices: [
            ...state.audioDevices.filter((d) => !d.virtualCable),
            ...(mockDriverInstalled
              ? [{ id: "SoundPush Microphone", name: "SoundPush Microphone", isInput: false, isDefault: false, virtualCable: true }]
              : []),
          ],
        });
        return undefined as T;
      }
      default:
        return undefined as T;
    }
  },
};
