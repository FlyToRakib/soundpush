// Typed client for the Tauri commands exposed by src-tauri/src/commands.rs.
// Outside Tauri (plain `vite` preview, tests) a mock engine is used instead.
import type {
  AuditEntry,
  AudioApps,
  DeviceProfile,
  DiagnosticsPreview,
  EngineState,
  HotkeyKind,
  HotkeyStatus,
  NetworkReport,
  NetworkStatus,
  PermissionKind,
  Policy,
  RouteKind,
  Settings,
  SettingsTopic,
  SystemStatus,
  TetheringStatus,
} from "./types";
import { mockEngine } from "./mock";

type Unlisten = () => void;

/** Android phones reachable over USB with adb (see src-tauri/src/usb.rs). */
export interface UsbStatus {
  adbFound: boolean;
  devices: { serial: string; model: string; authorized: boolean }[];
  /** Engine loopback TCP port the phone is forwarded to (0 = unavailable). */
  tcpPort: number;
}

/** Virtual microphone driver state (see src-tauri/src/virtual_mic.rs). */
export interface VirtualMicStatus {
  supported: boolean;
  /** Driver files installed; the device may still need a restart (Windows) to appear. */
  installed: boolean;
  /** "soundpush" = our own (macOS driver, Linux PipeWire/PulseAudio device), "vbcable" = VB-Audio's VB-CABLE (Windows). */
  provider: "soundpush" | "vbcable" | "none";
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) return mockEngine.invoke<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export async function onState(handler: (s: EngineState) => void): Promise<Unlisten> {
  if (!inTauri) return mockEngine.subscribe(handler);
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<EngineState>("engine://state", (e) => handler(e.payload));
  // Null while the engine is still starting; the first state event arrives when it is ready.
  const initial = await call<EngineState | null>("get_state");
  if (initial) handler(initial);
  return unlisten;
}

/** Why the engine could not start. Also reports a failure that happened before the UI loaded. */
export async function onStartError(handler: (message: string) => void): Promise<Unlisten> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<string>("engine://error", (e) => handler(e.payload));
  const earlier = await call<string | null>("get_start_error");
  if (earlier) handler(earlier);
  return unlisten;
}

/** The window was closed for the first time while SoundPush keeps running (`tray`: an icon is visible). */
export async function onCloseHint(handler: (tray: boolean) => void): Promise<Unlisten> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<boolean>("window://close-hint", (e) => handler(e.payload));
}

/** The OS reported a network change or a wake from sleep (src-tauri/src/os_events.rs). */
export async function onNetworkChanged(handler: () => void): Promise<Unlisten> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen("platform://network-changed", () => handler());
}

export const engine = {
  // pairing
  startPairing: () => call<string>("start_pairing"),
  stopPairing: () => call<void>("stop_pairing"),
  pairWithAddress: (address: string) => call<void>("pair_with_address", { address }),
  pairWithDevice: (deviceId: string) => call<void>("pair_with_device", { deviceId }),
  confirmPairing: (deviceId: string, accept: boolean) => call<void>("confirm_pairing", { deviceId, accept }),

  // devices
  connect: (deviceId: string) => call<void>("connect_device", { deviceId }),
  disconnect: (deviceId: string) => call<void>("disconnect_device", { deviceId }),
  forget: (deviceId: string) => call<void>("forget_device", { deviceId }),
  setBlocked: (deviceId: string, blocked: boolean) => call<void>("set_device_blocked", { deviceId, blocked }),
  rename: (deviceId: string, alias: string | null) => call<void>("rename_device", { deviceId, alias }),
  setAutoConnect: (deviceId: string, enabled: boolean) => call<void>("set_auto_connect", { deviceId, enabled }),
  setPermission: (deviceId: string, kind: PermissionKind, policy: Policy) =>
    call<void>("set_permission", { deviceId, kind, policy }),
  setDeviceProfile: (deviceId: string, profile: DeviceProfile | null) =>
    call<void>("set_device_profile", { deviceId, profile }),
  runNetworkTest: (deviceId: string) => call<NetworkReport>("run_network_test", { deviceId }),
  cancelNetworkTest: (deviceId: string) => call<void>("cancel_network_test", { deviceId }),
  usbStatus: () => call<UsbStatus>("usb_status"),
  usbConnect: (serial: string) => call<void>("usb_connect", { serial }),

  // routes
  /** `replace` takes the virtual microphone from the device feeding it now (plan §8.2); without
   *  it a second microphone route fails with `error.audio.virtualMicBusy`. */
  startRoute: (deviceId: string, kind: RouteKind, replace = false) =>
    call<string>("start_route", { deviceId, kind, replace }),
  stopRoute: (routeId: string) => call<void>("stop_route", { routeId }),
  setRouteVolume: (routeId: string, volume: number) => call<void>("set_route_volume", { routeId, volume }),
  setRouteMuted: (routeId: string, muted: boolean) => call<void>("set_route_muted", { routeId, muted }),
  setRouteKeepRunning: (routeId: string, keep: boolean) => call<void>("set_route_keep_running", { routeId, keep }),
  setPeerSpeakersMuted: (deviceId: string, muted: boolean) =>
    call<void>("set_peer_speakers_muted", { deviceId, muted }),
  respondRouteRequest: (requestId: number, accept: boolean, remember: boolean) =>
    call<void>("respond_route_request", { requestId, accept, remember }),

  // local audio
  setMicMuted: (muted: boolean) => call<void>("set_mic_muted", { muted }),
  setMicMonitor: (enabled: boolean) => call<void>("set_mic_monitor", { enabled }),
  refreshAudioDevices: () => call<void>("refresh_audio_devices"),
  /** Play a short tone on the chosen output, to check it is the right device (plan §5.1). */
  playTestTone: () => call<void>("play_test_tone"),
  virtualMicStatus: () => call<VirtualMicStatus>("virtual_mic_status"),
  installVirtualMic: () => call<void>("install_virtual_mic"),
  uninstallVirtualMic: () => call<void>("uninstall_virtual_mic"),
  restartComputer: () => call<void>("restart_computer"),
  listAudioApps: () => call<AudioApps>("list_audio_apps"),

  // desktop platform
  hotkeyStatus: () => call<HotkeyStatus>("hotkey_status"),
  setHotkey: (kind: HotkeyKind, accelerator: string | null) => call<void>("set_hotkey", { kind, accelerator }),
  networkStatus: () => call<NetworkStatus>("network_status"),
  fixFirewall: (includePublic: boolean) => call<NetworkStatus>("fix_firewall", { includePublic }),
  systemStatus: () => call<SystemStatus>("system_status"),
  tetheringStatus: () => call<TetheringStatus>("tethering_status"),
  requestMicrophone: () => call<void>("request_microphone"),
  openSystemSettings: (topic: SettingsTopic) => call<void>("open_system_settings", { topic }),

  // settings & app
  updateSettings: (settings: Settings) => call<Settings>("update_settings", { settings }),
  dismissNotice: (id: number) => call<void>("dismiss_notice", { id }),
  exportDiagnostics: () => call<string>("export_diagnostics"),
  previewDiagnostics: () => call<DiagnosticsPreview>("preview_diagnostics"),
  openLogsFolder: () => call<void>("open_logs_folder"),
  auditLog: () => call<AuditEntry[]>("get_audit_log"),
  clearAuditLog: () => call<void>("clear_audit_log"),
  openUrl: (url: string) => call<void>("open_url", { url }),
  closeMainWindow: () => call<void>("close_main_window"),
  quitApp: () => call<void>("quit_app"),
};
