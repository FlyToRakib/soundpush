import type { AudioDeviceView } from "./engine/types";

export const DEFAULT_DEVICE = "__default__";

export interface Option {
  value: string;
  label: string;
}

/**
 * Options for a device picker. A saved device that is not connected stays selected and is shown
 * as "(not available)" instead of the picker silently showing the system default.
 */
export function deviceOptions(
  devices: AudioDeviceView[],
  saved: string | null,
  defaultLabel: string,
  unavailableLabel: (name: string) => string,
): Option[] {
  const options = [{ value: DEFAULT_DEVICE, label: defaultLabel }, ...devices.map((d) => ({ value: d.id, label: d.name }))];
  if (saved && !devices.some((d) => d.id === saved)) options.push({ value: saved, label: unavailableLabel(saved) });
  return options;
}

/** Whether audio on this output goes over Bluetooth (the saved device, or the default). */
export function isBluetoothOutput(saved: string | null, defaultOutput: string | null, bluetooth: string[]): boolean {
  const name = saved ?? defaultOutput;
  return name !== null && bluetooth.includes(name);
}
