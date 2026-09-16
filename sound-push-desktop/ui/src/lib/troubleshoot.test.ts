import { describe, expect, it } from "vitest";
import type { EngineState, NetworkStatus, SystemStatus } from "./engine/types";
import { mockEngine } from "./engine/mock";
import { TOPICS, diagnose } from "./troubleshoot";

const network: NetworkStatus = {
  supported: true,
  firewallEnabled: true,
  blocked: false,
  publicNetwork: false,
  publicNetworkName: null,
  allowedOnPublic: false,
  canMakePrivate: false,
};
const system: SystemStatus = {
  microphone: "granted",
  systemAudio: "granted",
  mediaFeaturePackMissing: false,
  windowsEdition: "Professional",
  autostartDisabledByOs: false,
  bluetoothOutputs: [],
  defaultOutput: "Speakers",
  appCapture: true,
  webview2Version: "131.0.2903.112",
  audioEnhancements: [],
  vpn: { capturesInternet: false, name: null },
};

async function baseState(): Promise<EngineState> {
  return structuredClone(await mockEngine.invoke<EngineState>("get_state"));
}

describe("troubleshooter", () => {
  it("gives every topic at least one step", async () => {
    const state = await baseState();
    for (const topic of TOPICS) {
      expect(diagnose(topic, { state, network, system, virtualMicInstalled: false }).length).toBeGreaterThan(0);
    }
  });

  it("offers the firewall fix first when incoming connections are blocked", async () => {
    const state = await baseState();
    const steps = diagnose("noDevices", { state, network: { ...network, blocked: true }, system, virtualMicInstalled: null });
    expect(steps[0]).toMatchObject({ status: "problem", action: "fixFirewall", key: "trouble.check.firewallBlocked" });
  });

  it("names the public network, because that is what the fix has to deal with", async () => {
    const state = await baseState();
    const blocked = { ...network, blocked: true, publicNetwork: true, publicNetworkName: "Loops 3", canMakePrivate: true };
    const steps = diagnose("noDevices", { state, network: blocked, system, virtualMicInstalled: null });
    expect(steps[0]).toMatchObject({ key: "trouble.check.firewallBlockedPublic", args: ["Loops 3"], action: "fixFirewall" });
  });

  it("explains a missing virtual microphone and a pending restart", async () => {
    const state = await baseState();
    expect(diagnose("micApps", { state, network, system, virtualMicInstalled: false })[0]?.key).toBe("trouble.check.virtualMicMissing");
    expect(diagnose("micApps", { state, network, system, virtualMicInstalled: true })[0]?.key).toBe("trouble.check.virtualMicRestart");
  });

  it("points out Bluetooth output delay", async () => {
    const state = await baseState();
    const steps = diagnose("crackles", { state, network, system: { ...system, bluetoothOutputs: ["Speakers"] }, virtualMicInstalled: null });
    expect(steps.some((s) => s.key === "trouble.check.bluetooth")).toBe(true);
  });

  it("reports an unplugged saved output device", async () => {
    const state = await baseState();
    state.settings.output.device = "USB Headset";
    const steps = diagnose("noSound", { state, network, system, virtualMicInstalled: null });
    expect(steps.find((s) => s.key === "trouble.check.deviceMissing")?.args).toEqual(["USB Headset"]);
  });

  it("warns about a VPN that carries the whole connection", async () => {
    const state = await baseState();
    const vpn = { ...system, vpn: { capturesInternet: true, name: "NordLynx" } };
    for (const topic of ["noDevices", "disconnects"] as const) {
      const steps = diagnose(topic, { state, network, system: vpn, virtualMicInstalled: null });
      expect(steps.find((s) => s.key === "trouble.check.vpn")?.args).toEqual(["NordLynx"]);
    }
    // A VPN with a route of its own leaves the local network alone and is not mentioned.
    const steps = diagnose("noDevices", { state, network, system, virtualMicInstalled: null });
    expect(steps.some((s) => s.key === "trouble.check.vpn")).toBe(false);
  });

  it("diagnoses a network that isolates its clients instead of the static tip", async () => {
    const state = await baseState();
    state.local.addresses = ["192.168.1.5"];
    // Paired, known to be on this very network, and still unreachable.
    state.peers = state.peers.map((p) => ({
      ...p,
      trusted: true,
      online: false,
      connection: "waitingForDevice",
      addresses: ["192.168.1.42"],
    }));
    const steps = diagnose("noDevices", { state, network, system, virtualMicInstalled: null });
    expect(steps.some((s) => s.key === "trouble.check.isolated" && s.action === "openUsb")).toBe(true);
    expect(steps.some((s) => s.key === "trouble.check.guestNetwork")).toBe(false);

    // The same picture with a blocked firewall has a better answer, so isolation is not claimed.
    const blocked = diagnose("noDevices", { state, network: { ...network, blocked: true }, system, virtualMicInstalled: null });
    expect(blocked.some((s) => s.key === "trouble.check.isolated")).toBe(false);
    expect(blocked.some((s) => s.key === "trouble.check.guestNetwork")).toBe(true);
  });

  it("keeps the static guest-network tip when the devices are on another network", async () => {
    const state = await baseState();
    state.local.addresses = ["192.168.1.5"];
    state.peers = state.peers.map((p) => ({
      ...p,
      trusted: true,
      online: false,
      connection: "waitingForDevice",
      // A different subnet: the phone is elsewhere, not isolated.
      addresses: ["10.0.0.9"],
    }));
    const steps = diagnose("noDevices", { state, network, system, virtualMicInstalled: null });
    expect(steps.some((s) => s.key === "trouble.check.isolated")).toBe(false);
    expect(steps.some((s) => s.key === "trouble.check.guestNetwork")).toBe(true);
  });

  it("mentions audio enhancement software when sound is missing or crackling", async () => {
    const state = await baseState();
    const nahimic = { ...system, audioEnhancements: ["Nahimic", "Sonic Studio 3"] };
    for (const topic of ["noSound", "crackles", "micApps"] as const) {
      const steps = diagnose(topic, { state, network, system: nahimic, virtualMicInstalled: true });
      expect(steps.find((s) => s.key === "trouble.check.audioEnhancements")?.args).toEqual(["Nahimic, Sonic Studio 3"]);
    }
    // Nothing of the sort running: no advice about software the user does not have.
    const steps = diagnose("noSound", { state, network, system, virtualMicInstalled: null });
    expect(steps.some((s) => s.key === "trouble.check.audioEnhancements")).toBe(false);
  });
});
