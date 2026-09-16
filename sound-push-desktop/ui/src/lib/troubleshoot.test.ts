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
    expect(diagnose("micApps", { state, network, system, virtualMicInstalled: false })[0]?.key).toBe(
      "trouble.check.virtualMicMissing",
    );
    expect(diagnose("micApps", { state, network, system, virtualMicInstalled: true })[0]?.key).toBe(
      "trouble.check.virtualMicRestart",
    );
  });

  it("points out Bluetooth output delay", async () => {
    const state = await baseState();
    const steps = diagnose("crackles", {
      state,
      network,
      system: { ...system, bluetoothOutputs: ["Speakers"] },
      virtualMicInstalled: null,
    });
    expect(steps.some((s) => s.key === "trouble.check.bluetooth")).toBe(true);
  });

  it("reports an unplugged saved output device", async () => {
    const state = await baseState();
    state.settings.output.device = "USB Headset";
    const steps = diagnose("noSound", { state, network, system, virtualMicInstalled: null });
    expect(steps.find((s) => s.key === "trouble.check.deviceMissing")?.args).toEqual(["USB Headset"]);
  });
});
