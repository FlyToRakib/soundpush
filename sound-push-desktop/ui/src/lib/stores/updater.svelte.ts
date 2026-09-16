// Update state shared by the update banner and Settings → About.
//
// Background checks run at most once a day while enabled and stay silent when offline or failing.
// Checks the user starts show their result ("up to date", or what went wrong).
// The update channel (stable/beta) comes from the settings through `configure`.
//
// Two rules keep background checks out of the way (plan §24): after the OS starts SoundPush at
// sign-in the first check waits half a minute, and a metered connection (a phone hotspot, a
// capped plan) is left alone entirely. A check the user asks for always runs.
import { untrack } from "svelte";
import { engine } from "../engine/client";
import { checkForUpdate, relaunch, type AvailableUpdate, type CheckOptions } from "../engine/updater";
import type { UpdateChannel, UpdateConditions } from "../engine/types";

export type UpdateStatus = "idle" | "checking" | "upToDate" | "available" | "downloading" | "ready" | "error";

const LAST_CHECK_KEY = "sp-update-last-check";
export const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
/** How often a running window looks whether the daily check is due. */
const POLL_MS = 60 * 60 * 1000;
/** How long the first background check waits when the OS started SoundPush at sign-in. */
export const AUTOSTART_DELAY_MS = 30_000;

/** Neither rule applies when the shell cannot say — an update check is not worth an error. */
const NO_CONDITIONS: UpdateConditions = { autostarted: false, metered: false };

async function askConditions(): Promise<UpdateConditions> {
  try {
    return (await engine.updateConditions()) ?? NO_CONDITIONS;
  } catch {
    return NO_CONDITIONS;
  }
}

function readLastCheck(): number {
  try {
    return Number(localStorage.getItem(LAST_CHECK_KEY)) || 0;
  } catch {
    return 0;
  }
}

function writeLastCheck(ms: number): void {
  try {
    localStorage.setItem(LAST_CHECK_KEY, String(ms));
  } catch {
    // Storage unavailable: the next window simply checks again.
  }
}

export class UpdaterStore {
  status = $state<UpdateStatus>("idle");
  version = $state<string | null>(null);
  notes = $state("");
  downloaded = $state(0);
  total = $state<number | null>(null);
  /** i18n key of the last error from something the user did; background failures never set it. */
  errorKey = $state<string | null>(null);
  /** The banner was closed with "Later"; Settings still shows the update. */
  dismissed = $state(false);

  private update: AvailableUpdate | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private firstCheck: ReturnType<typeof setTimeout> | null = null;
  // Plain fields (not $state): reading them inside an effect must not subscribe it.
  private channel: UpdateChannel = "stable";
  private installId = "";
  private configured = false;

  constructor(
    private readonly checker: (options: CheckOptions) => Promise<AvailableUpdate | null> = checkForUpdate,
    private readonly restartApp: () => Promise<void> = relaunch,
    private readonly now: () => number = Date.now,
    private readonly conditions: () => Promise<UpdateConditions> = askConditions,
  ) {}

  get busy(): boolean {
    return this.status === "checking" || this.status === "downloading";
  }

  /**
   * Update channel and install id from the settings. Switching channel forgets an offered update and,
   * while automatic checks are on, checks that channel once on the next tick; afterwards the daily gate applies.
   */
  configure(channel: UpdateChannel, installId: string): void {
    untrack(() => {
      this.installId = installId;
      const switched = this.configured && channel !== this.channel;
      this.channel = channel;
      this.configured = true;
      if (!switched || this.busy || this.status === "ready") return;
      this.update = null;
      this.version = null;
      this.notes = "";
      this.status = "idle";
      this.dismissed = false;
      if (this.timer) setTimeout(() => void this.check(false), 0);
    });
  }

  /** Start or stop daily background checks (Settings → "Check for updates automatically"). */
  setAutomatic(enabled: boolean): void {
    if (this.timer) clearInterval(this.timer);
    if (this.firstCheck) clearTimeout(this.firstCheck);
    this.timer = null;
    this.firstCheck = null;
    if (!enabled) return;
    this.timer = setInterval(() => void this.checkIfDue(), POLL_MS);
    // Called from a component effect: start the check on the next tick so the state it reads
    // is not tracked by that effect (tracking it re-ran the effect on every status change).
    this.firstCheck = setTimeout(() => void this.scheduleFirstCheck(), 0);
  }

  /** The first background check of a run, delayed when the OS started SoundPush at sign-in. */
  private async scheduleFirstCheck(): Promise<void> {
    this.firstCheck = null;
    const { autostarted } = await this.conditions();
    // Switched off while the shell was answering.
    if (!this.timer) return;
    if (!autostarted) {
      await this.checkIfDue();
      return;
    }
    this.firstCheck = setTimeout(() => {
      this.firstCheck = null;
      void this.checkIfDue();
    }, AUTOSTART_DELAY_MS);
  }

  async checkIfDue(): Promise<void> {
    if (this.now() - readLastCheck() < CHECK_INTERVAL_MS) return;
    // Someone's mobile data is not for a download SoundPush decided to make on its own. The next
    // poll looks again, and "Check now" in Settings still works.
    if ((await this.conditions()).metered) return;
    await this.check(false);
  }

  async check(manual: boolean): Promise<void> {
    // Never interrupt a download, and keep a finished one.
    if (this.busy || this.status === "ready") return;
    const before = this.status;
    this.status = "checking";
    if (manual) this.errorKey = null;
    // A background attempt counts even when it fails, so an unreachable or missing update
    // feed is tried again after the daily interval, not immediately.
    if (!manual) writeLastCheck(this.now());
    try {
      const update = await this.checker({ channel: this.channel, installId: this.installId, manual });
      writeLastCheck(this.now());
      if (update) {
        const isNew = update.version !== this.version;
        this.update = update;
        this.version = update.version;
        this.notes = update.notes;
        this.status = "available";
        if (manual || isNew) this.dismissed = false;
      } else {
        this.update = null;
        this.version = null;
        this.status = manual ? "upToDate" : "idle";
      }
    } catch {
      if (manual) {
        this.status = "error";
        this.errorKey = "update.checkFailed";
      } else {
        // Offline or GitHub unreachable: stay quiet and keep whatever was known before.
        this.status = before === "available" ? "available" : "idle";
      }
    }
  }

  async download(): Promise<void> {
    const update = this.update;
    if (!update || this.status !== "available") return;
    this.status = "downloading";
    this.errorKey = null;
    this.downloaded = 0;
    this.total = null;
    try {
      await update.download((downloaded, total) => {
        this.downloaded = downloaded;
        this.total = total;
      });
      this.status = "ready";
    } catch {
      this.status = "available";
      this.errorKey = "update.downloadFailed";
    }
  }

  /** Install the downloaded update and start the new version. */
  async restart(): Promise<void> {
    const update = this.update;
    if (!update || this.status !== "ready") return;
    this.errorKey = null;
    try {
      await update.install();
      await this.restartApp();
    } catch {
      this.errorKey = "update.installFailed";
    }
  }

  dismiss(): void {
    this.dismissed = true;
  }

  /** Download progress 0–100, or null while the size is unknown. */
  get percent(): number | null {
    if (!this.total) return null;
    return Math.min(100, Math.round((this.downloaded / this.total) * 100));
  }
}

export const updater = new UpdaterStore();
