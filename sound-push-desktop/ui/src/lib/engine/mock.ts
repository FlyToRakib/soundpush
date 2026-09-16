// In-browser mock engine so the UI can be previewed and tested without Tauri.
import type { AuditEntry, DeviceProfile, EngineState, NetworkReport, RouteKind, Settings } from "./types";

const auditLog: AuditEntry[] = [
  { timeUnix: Date.now() / 1000 - 60, kind: "routeStarted", peerName: "Pixel 8", peerCode: "SP-7F3A-19C2-4B8E", route: "sendSystemAudio", detail: "peer" },
  { timeUnix: Date.now() / 1000 - 3600, kind: "permissionChanged", peerName: "Pixel 8", peerCode: "SP-7F3A-19C2-4B8E", detail: "useMyMicrophone=ask" },
  { timeUnix: Date.now() / 1000 - 7200, kind: "pairingSucceeded", peerName: "Pixel 8", peerCode: "SP-7F3A-19C2-4B8E", detail: "" },
];

const settings: Settings = {
  version: 2,
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
    highPass: true,
  },
  capture: { systemDevice: null, muteLocalSpeakers: false, app: null, excludeApp: false },
  desktop: {
    launchAtLogin: true,
    startMinimized: true,
    closeToTray: true,
    preventSleepWhileStreaming: false,
    muteHotkey: null,
    pushToTalkHotkey: null,
    virtualMicDevice: null,
    autoStartMic: false,
    lastMicPeer: null,
  },
  mobile: { stayAvailable: false, remindAfterRestart: true },
  autoConnectTrusted: true,
  resumeRoutesOnStart: false,
  savedRoutes: [],
  dismissedTips: [],
  audioCues: false,
  deviceProfiles: {},
  checkForUpdates: true,
  updateChannel: "stable",
  debugLogging: false,
  debugLoggingUntilUnix: 0,
};

let state: EngineState = {
  revision: 1,
  local: {
    deviceId: "00".repeat(16),
    displayCode: "SP-0000-0000-0000",
    name: settings.deviceName,
    platform: "windows",
    port: 47650,
    tcpPort: 47650,
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
      transport: "quic",
      remoteAddress: "192.168.1.23:47650",
      speakersMuted: false,
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
  micMuted: false,
  networkTests: [],
};

const mockNetwork = {
  supported: true,
  firewallEnabled: true,
  blocked: false,
  publicNetwork: false,
  publicNetworkName: null,
  allowedOnPublic: false,
  canMakePrivate: false,
};

const mockReport: NetworkReport = {
  transport: "quic",
  rttMs: 4.2,
  rttP95Ms: 7.9,
  jitterMs: 1.1,
  lossPct: 0,
  achievableKbps: 1600,
  maxDatagramBytes: 1162,
  probesSent: 420,
  probesReceived: 420,
  durationMs: 9400,
  recommendation: {
    latency: "lowLatency",
    quality: "auto",
    opusBitrate: 128000,
    redundancy: false,
    tips: ["nettest.tip.good", "nettest.tip.losslessOk"],
  },
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
              stats: {
                codec: "Opus",
                bitrateKbps: 128,
                latencyMs: 52,
                bufferMs: 30,
                jitterMs: 2,
                lossPct: 0,
                underruns: 0,
                driftPpm: 12,
                levelDb: -18,
                captureMs: 10,
                encodeMs: 10,
                networkMs: 2,
                outputMs: 0,
              },
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
      case "get_audit_log":
        return [...auditLog] as T;
      case "clear_audit_log":
        auditLog.splice(0, auditLog.length, { timeUnix: Date.now() / 1000, kind: "logCleared", peerName: "", peerCode: "", detail: "" });
        return undefined as T;
      case "preview_diagnostics":
        return {
          sections: [
            { id: "system", count: 0, redacted: [] },
            { id: "state", count: state.peers.length, redacted: ["addresses", "deviceIds", "pairingCode"] },
            { id: "log", count: 2, redacted: ["addresses", "deviceIds"] },
          ],
          text: "SoundPush 0.1.0 diagnostics\nOS: windows x86_64\n\n== State ==\n{}\n\n== Recent log ==\nINFO SoundPush starting\nINFO listening on <address>\n",
        } as T;
      case "set_device_profile": {
        const profiles = { ...state.settings.deviceProfiles };
        const profile = args?.profile as DeviceProfile | null;
        if (profile && Object.keys(profile).length > 0) profiles[String(args?.deviceId)] = profile;
        else delete profiles[String(args?.deviceId)];
        emit({ settings: { ...state.settings, deviceProfiles: profiles } });
        return undefined as T;
      }
      case "run_network_test": {
        const peerId = String(args?.deviceId);
        const others = state.networkTests.filter((t) => t.peerId !== peerId);
        emit({ networkTests: [...others, { peerId, status: "done", progress: 1, report: mockReport, error: null, startedUnix: Date.now() / 1000 }] });
        return mockReport as T;
      }
      case "usb_status":
        return { adbFound: true, devices: [{ serial: "mock123", model: "Redmi Note 9 Pro", authorized: true }], tcpPort: state.local.tcpPort } as T;
      case "set_peer_speakers_muted":
        emit({ peers: state.peers.map((p) => (p.deviceId === args?.deviceId ? { ...p, speakersMuted: Boolean(args?.muted) } : p)) });
        return undefined as T;
      case "set_mic_muted":
        emit({ micMuted: Boolean(args?.muted) });
        return undefined as T;
      case "network_status":
      case "fix_firewall":
        return { ...mockNetwork } as T;
      case "system_status":
        return {
          microphone: "granted",
          systemAudio: "granted",
          mediaFeaturePackMissing: false,
          windowsEdition: "Professional",
          autostartDisabledByOs: false,
          bluetoothOutputs: [],
          defaultOutput: "Speakers",
          appCapture: true,
        } as T;
      case "tethering_status":
        return { active: false, internetViaPhone: false, peers: [] } as T;
      case "hotkey_status":
        return { mute: null, pushToTalk: null } as T;
      case "set_hotkey": {
        const next = structuredClone(state.settings);
        const accelerator = (args?.accelerator as string | null) ?? null;
        if (args?.kind === "mute") next.desktop.muteHotkey = accelerator;
        else next.desktop.pushToTalkHotkey = accelerator;
        emit({ settings: next });
        return undefined as T;
      }
      case "list_audio_apps":
        return { supported: true, apps: [{ process: "spotify.exe", active: true }, { process: "chrome.exe", active: false }] } as T;
      case "virtual_mic_status":
        return { supported: true, installed: mockDriverInstalled, provider: "soundpush" } as T;
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
