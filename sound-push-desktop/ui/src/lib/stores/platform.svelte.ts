// OS status the desktop shell reports (firewall, permissions, media components, shortcuts).
// Refreshed when the window opens and every minute while it stays open.
import { engine } from "../engine/client";
import type { HotkeyStatus, NetworkStatus, SystemStatus } from "../engine/types";

const REFRESH_MS = 60_000;

class PlatformStore {
  network = $state<NetworkStatus | null>(null);
  system = $state<SystemStatus | null>(null);
  hotkeys = $state<HotkeyStatus>({ mute: null, pushToTalk: null });
  private started = false;

  /** Start periodic checks (idempotent). */
  start(): void {
    if (this.started) return;
    this.started = true;
    void this.refresh();
    setInterval(() => void this.refresh(), REFRESH_MS);
  }

  async refresh(): Promise<void> {
    const [network, system, hotkeys] = await Promise.allSettled([engine.networkStatus(), engine.systemStatus(), engine.hotkeyStatus()]);
    if (network.status === "fulfilled" && network.value) this.network = network.value;
    if (system.status === "fulfilled" && system.value) this.system = system.value;
    if (hotkeys.status === "fulfilled" && hotkeys.value) this.hotkeys = hotkeys.value;
  }
}

export const platform = new PlatformStore();
