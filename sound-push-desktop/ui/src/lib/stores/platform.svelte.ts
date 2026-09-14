// OS status the desktop shell reports (firewall, permissions, media components, shortcuts,
// USB tethering). Refreshed when the window opens, every minute while it stays open, and when
// the OS reports a network change or a wake from sleep.
import { engine, onNetworkChanged } from "../engine/client";
import type { HotkeyStatus, NetworkStatus, SystemStatus, TetheringStatus } from "../engine/types";

const REFRESH_MS = 60_000;

class PlatformStore {
  network = $state<NetworkStatus | null>(null);
  system = $state<SystemStatus | null>(null);
  hotkeys = $state<HotkeyStatus>({ mute: null, pushToTalk: null });
  tethering = $state<TetheringStatus | null>(null);
  private started = false;

  /** Start periodic checks (idempotent). */
  start(): void {
    if (this.started) return;
    this.started = true;
    void this.refresh();
    setInterval(() => void this.refresh(), REFRESH_MS);
    void onNetworkChanged(() => void this.refresh());
  }

  async refresh(): Promise<void> {
    const [network, system, hotkeys, tethering] = await Promise.allSettled([
      engine.networkStatus(),
      engine.systemStatus(),
      engine.hotkeyStatus(),
      engine.tetheringStatus(),
    ]);
    if (network.status === "fulfilled" && network.value) this.network = network.value;
    if (system.status === "fulfilled" && system.value) this.system = system.value;
    if (hotkeys.status === "fulfilled" && hotkeys.value) this.hotkeys = hotkeys.value;
    if (tethering.status === "fulfilled" && tethering.value) this.tethering = tethering.value;
  }
}

export const platform = new PlatformStore();
