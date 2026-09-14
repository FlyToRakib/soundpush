// Navigation and app-wide dialogs, so notices and the troubleshooter can lead to a fix.
import type { Topic } from "../troubleshoot";

export type Page = "home" | "devices" | "audio" | "settings";

class UiStore {
  page = $state<Page>("home");
  /** The firewall fix dialog is open. */
  firewallFix = $state(false);
  /** Open the troubleshooter on this topic (Settings page). */
  troubleshoot = $state<Topic | null>(null);
}

export const ui = new UiStore();
