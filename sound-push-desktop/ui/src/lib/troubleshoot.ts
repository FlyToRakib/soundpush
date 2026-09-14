// Guided troubleshooter (plan §28.2): each topic runs automatic checks against the engine state
// and the OS status before asking the user anything. Pure logic, so it is unit-tested.
import type { EngineState, NetworkStatus, SystemStatus } from "./engine/types";
import { isConnected } from "./devices";

export type Topic = "noDevices" | "disconnects" | "noSound" | "micApps" | "crackles";
export const TOPICS: Topic[] = ["noDevices", "disconnects", "noSound", "micApps", "crackles"];

export type StepAction =
  | "pair"
  | "fixFirewall"
  | "openNetworkSettings"
  | "openMicSettings"
  | "openSystemAudioSettings"
  | "openSoundSettings"
  | "optionalFeatures"
  | "openAudio"
  | "setStable"
  | "preventSleep"
  | "unmuteMic"
  | "useAutoVirtualMic"
  | "visibilityTrusted";

export interface Step {
  /** "problem" needs action, "tip" is advice, "ok" is a check that passed. */
  status: "ok" | "problem" | "tip";
  /** i18n key (`trouble.check.*`). */
  key: string;
  args?: string[];
  action?: StepAction;
}

export interface Context {
  state: EngineState;
  network: NetworkStatus | null;
  system: SystemStatus | null;
  /** Virtual microphone driver files present (may still need a restart). */
  virtualMicInstalled: boolean | null;
}

const names = (list: { name: string }[]) => list.map((p) => p.name).join(", ");

function firewall(ctx: Context): Step[] {
  const n = ctx.network;
  if (!n?.supported) return [];
  if (n.firewallEnabled && n.blocked) return [{ status: "problem", key: "trouble.check.firewallBlocked", action: "fixFirewall" }];
  const steps: Step[] = [{ status: "ok", key: "trouble.check.firewallOk" }];
  if (n.publicNetwork && !n.allowedOnPublic) {
    steps.push({ status: "tip", key: "trouble.check.publicNetwork", args: [n.publicNetworkName ?? ""], action: "openNetworkSettings" });
  }
  return steps;
}

function bluetooth(ctx: Context): Step[] {
  const s = ctx.state.settings;
  const output = s.output.device ?? ctx.system?.defaultOutput ?? null;
  return output && ctx.system?.bluetoothOutputs.includes(output)
    ? [{ status: "tip", key: "trouble.check.bluetooth", args: [output], action: s.stream.latency === "stable" ? undefined : "setStable" }]
    : [];
}

export function diagnose(topic: Topic, ctx: Context): Step[] {
  const { state } = ctx;
  const s = state.settings;
  const trusted = state.peers.filter((p) => p.trusted);
  const connected = trusted.filter((p) => isConnected(p.connection));
  const poor = connected.filter((p) => p.quality === "poor");
  const active = state.routes.filter((r) => r.status === "active");
  const steps: Step[] = [];

  switch (topic) {
    case "noDevices":
      if (state.local.addresses.length === 0) steps.push({ status: "problem", key: "trouble.check.noNetwork", action: "openNetworkSettings" });
      steps.push(...firewall(ctx));
      if (s.visibility === "hidden") steps.push({ status: "problem", key: "trouble.check.hidden", action: "visibilityTrusted" });
      if (trusted.length === 0) steps.push({ status: "problem", key: "trouble.check.notPaired", action: "pair" });
      else if (connected.length === 0) steps.push({ status: "problem", key: "trouble.check.notConnected" });
      else steps.push({ status: "ok", key: "trouble.check.connected", args: [names(connected)] });
      steps.push({ status: "tip", key: "trouble.check.guestNetwork" });
      break;

    case "disconnects":
      if (poor.length > 0) steps.push({ status: "problem", key: "trouble.check.weakSignal", args: [names(poor)] });
      steps.push(...firewall(ctx).filter((step) => step.status !== "ok"));
      if (s.stream.latency !== "stable") steps.push({ status: "tip", key: "trouble.check.useStable", action: "setStable" });
      if (!s.desktop.preventSleepWhileStreaming) steps.push({ status: "tip", key: "trouble.check.sleep", action: "preventSleep" });
      if (!steps.some((step) => step.status === "problem")) steps.unshift({ status: "ok", key: "trouble.check.connectionOk" });
      break;

    case "noSound": {
      const outputs = state.audioDevices.filter((d) => !d.isInput && !d.virtualCable);
      if (active.length === 0) steps.push({ status: "tip", key: "trouble.check.noStream" });
      const muted = active.filter((r) => r.muted);
      if (muted.length > 0) steps.push({ status: "problem", key: "trouble.check.routeMuted", args: [muted.map((r) => r.peerName).join(", ")] });
      if (outputs.length === 0) steps.push({ status: "problem", key: "trouble.check.noOutputDevice", action: "openSoundSettings" });
      for (const saved of [s.output.device, s.capture.systemDevice]) {
        if (saved && !outputs.some((d) => d.id === saved)) {
          steps.push({ status: "problem", key: "trouble.check.deviceMissing", args: [saved], action: "openAudio" });
        }
      }
      if (s.output.volume === 0) steps.push({ status: "problem", key: "trouble.check.volumeZero", action: "openAudio" });
      if (ctx.system?.systemAudio === "denied") {
        steps.push({ status: "problem", key: "trouble.check.systemAudioDenied", action: "openSystemAudioSettings" });
      }
      if (ctx.system?.mediaFeaturePackMissing) steps.push({ status: "problem", key: "trouble.check.mediaFeaturePack", action: "optionalFeatures" });
      if (s.capture.app) {
        steps.push({ status: "tip", key: s.capture.excludeApp ? "trouble.check.appExcluded" : "trouble.check.appOnly", args: [s.capture.app], action: "openAudio" });
      }
      steps.push(...bluetooth(ctx));
      if (!steps.some((step) => step.status === "problem")) steps.unshift({ status: "ok", key: "trouble.check.audioOk" });
      break;
    }

    case "micApps": {
      const caps = state.capabilities;
      if (!caps.virtualMic) {
        steps.push({
          status: "problem",
          key: ctx.virtualMicInstalled ? "trouble.check.virtualMicRestart" : "trouble.check.virtualMicMissing",
          action: "openAudio",
        });
      } else {
        const chosen = s.desktop.virtualMicDevice;
        if (chosen && chosen !== caps.virtualMicDevice) {
          steps.push({ status: "problem", key: "trouble.check.virtualMicIgnored", args: [chosen], action: "useAutoVirtualMic" });
        }
        steps.push({ status: "ok", key: "trouble.check.chooseInput", args: [caps.virtualMicInput ?? caps.virtualMicDevice ?? ""] });
      }
      if (state.micMuted) steps.push({ status: "problem", key: "trouble.check.micMuted", action: "unmuteMic" });
      if (ctx.system?.microphone === "denied") steps.push({ status: "problem", key: "trouble.check.micDenied", action: "openMicSettings" });
      if (!active.some((r) => r.kind === "receiveMicToVirtualMic")) steps.push({ status: "tip", key: "trouble.check.startMic" });
      break;
    }

    case "crackles":
      if (poor.length > 0) steps.push({ status: "problem", key: "trouble.check.weakSignal", args: [names(poor)] });
      if (active.some((r) => r.stats.underruns > 50)) steps.push({ status: "problem", key: "trouble.check.dropouts", action: s.stream.latency === "stable" ? undefined : "setStable" });
      if (s.stream.latency !== "stable") steps.push({ status: "tip", key: "trouble.check.useStable", action: "setStable" });
      steps.push(...bluetooth(ctx));
      if (s.mic.noiseSuppression) steps.push({ status: "tip", key: "trouble.check.noiseSuppression" });
      steps.push({ status: "tip", key: "trouble.check.closer" });
      break;
  }
  // Problems first, then tips, then passed checks.
  const order = { problem: 0, tip: 1, ok: 2 };
  return steps.sort((a, b) => order[a.status] - order[b.status]);
}
