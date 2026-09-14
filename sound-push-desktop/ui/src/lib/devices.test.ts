import { describe, expect, it } from "vitest";
import { DEFAULT_DEVICE, deviceOptions, isBluetoothOutput, isConnected } from "./devices";

const speakers = { id: "Speakers", name: "Speakers", isInput: false, isDefault: true, virtualCable: false };

describe("device options", () => {
  it("lists the default and connected devices", () => {
    expect(deviceOptions([speakers], null, "System default", (n) => `${n} (not available)`)).toEqual([
      { value: DEFAULT_DEVICE, label: "System default" },
      { value: "Speakers", label: "Speakers" },
    ]);
  });

  it("keeps an unplugged saved device visible", () => {
    const options = deviceOptions([speakers], "USB Headset", "System default", (n) => `${n} (not available)`);
    expect(options.at(-1)).toEqual({ value: "USB Headset", label: "USB Headset (not available)" });
  });

  it("detects Bluetooth output from the saved or default device", () => {
    expect(isBluetoothOutput(null, "AirPods", ["AirPods"])).toBe(true);
    expect(isBluetoothOutput("Speakers", "AirPods", ["AirPods"])).toBe(false);
    expect(isBluetoothOutput(null, null, ["AirPods"])).toBe(false);
  });

  it("treats a degraded connection as connected", () => {
    expect(isConnected("connected")).toBe(true);
    expect(isConnected("degraded")).toBe(true);
    expect(isConnected("reconnecting")).toBe(false);
    expect(isConnected("disconnected")).toBe(false);
  });
});
