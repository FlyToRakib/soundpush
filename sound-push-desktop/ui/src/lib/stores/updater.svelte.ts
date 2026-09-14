// Update state shared by the update banner and Settings → About.
//
// Background checks run at most once a day while enabled and stay silent when offline or failing.
// Checks the user starts show their result ("up to date", or what went wrong).
import { checkForUpdate, relaunch, type AvailableUpdate } from "../engine/updater";

export type UpdateStatus = "idle" | "checking" | "upToDate" | "available" | "downloading" | "ready" | "error";

const LAST_CHECK_KEY = "sp-update-last-check";
export const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
/** How often a running window looks whether the daily check is due. */
const POLL_MS = 60 * 60 * 1000;

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

  constructor(
    private readonly checker: () => Promise<AvailableUpdate | null> = checkForUpdate,
    private readonly restartApp: () => Promise<void> = relaunch,
    private readonly now: () => number = Date.now,
  ) {}

  get busy(): boolean {
    return this.status === "checking" || this.status === "downloading";
  }

  /** Start or stop daily background checks (Settings → "Check for updates automatically"). */
  setAutomatic(enabled: boolean): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    if (!enabled) return;
    void this.checkIfDue();
    this.timer = setInterval(() => void this.checkIfDue(), POLL_MS);
  }

  async checkIfDue(): Promise<void> {
    if (this.now() - readLastCheck() >= CHECK_INTERVAL_MS) await this.check(false);
  }

  async check(manual: boolean): Promise<void> {
    // Never interrupt a download, and keep a finished one.
    if (this.busy || this.status === "ready") return;
    const before = this.status;
    this.status = "checking";
    if (manual) this.errorKey = null;
    try {
      const update = await this.checker();
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
