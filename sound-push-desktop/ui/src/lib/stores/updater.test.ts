import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UpdateConditions } from "../engine/types";
import type { AvailableUpdate } from "../engine/updater";
import { AUTOSTART_DELAY_MS, CHECK_INTERVAL_MS, UpdaterStore } from "./updater.svelte";

/** The ordinary case: opened by the user, on a connection nobody pays by the megabyte. */
const openedByUser = async (): Promise<UpdateConditions> => ({ autostarted: false, metered: false });

function fakeUpdate(overrides: Partial<AvailableUpdate> = {}): AvailableUpdate {
  return {
    version: "9.9.9",
    notes: "Better things",
    download: async (onProgress) => {
      onProgress(50, 100);
      onProgress(100, 100);
    },
    install: async () => {},
    ...overrides,
  };
}

beforeEach(() => {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("UpdaterStore", () => {
  it("stays silent when a background check fails", async () => {
    const store = new UpdaterStore(() => Promise.reject(new Error("offline")));
    await store.check(false);
    expect(store.status).toBe("idle");
    expect(store.errorKey).toBeNull();
  });

  it("explains a failed check the user started", async () => {
    const store = new UpdaterStore(() => Promise.reject(new Error("offline")));
    await store.check(true);
    expect(store.status).toBe("error");
    expect(store.errorKey).toBe("update.checkFailed");
  });

  it("clears the error on the next manual check", async () => {
    let fail = true;
    const store = new UpdaterStore(() => (fail ? Promise.reject(new Error("offline")) : Promise.resolve(null)));
    await store.check(true);
    fail = false;
    await store.check(true);
    expect(store.status).toBe("upToDate");
    expect(store.errorKey).toBeNull();
  });

  it("does not say 'up to date' for a background check", async () => {
    const store = new UpdaterStore(() => Promise.resolve(null));
    await store.check(false);
    expect(store.status).toBe("idle");
  });

  it("downloads with progress, then installs and restarts", async () => {
    const install = vi.fn(async () => {});
    const restart = vi.fn(async () => {});
    const store = new UpdaterStore(() => Promise.resolve(fakeUpdate({ install })), restart);
    await store.check(false);
    expect(store.status).toBe("available");
    expect(store.version).toBe("9.9.9");

    await store.download();
    expect(store.status).toBe("ready");
    expect(store.percent).toBe(100);

    await store.restart();
    expect(install).toHaveBeenCalledOnce();
    expect(restart).toHaveBeenCalledOnce();
  });

  it("keeps the update available when the download fails", async () => {
    const store = new UpdaterStore(() =>
      Promise.resolve(fakeUpdate({ download: () => Promise.reject(new Error("connection reset")) })),
    );
    await store.check(true);
    await store.download();
    expect(store.status).toBe("available");
    expect(store.errorKey).toBe("update.downloadFailed");
  });

  it("keeps a known update when a later background check fails", async () => {
    let fail = false;
    const store = new UpdaterStore(() => (fail ? Promise.reject(new Error("offline")) : Promise.resolve(fakeUpdate())));
    await store.check(false);
    fail = true;
    await store.check(false);
    expect(store.status).toBe("available");
  });

  it("checks in the background at most once a day", async () => {
    let now = 1_000_000_000_000;
    const checker = vi.fn(() => Promise.resolve(null));
    const store = new UpdaterStore(
      checker,
      async () => {},
      () => now,
      openedByUser,
    );
    await store.checkIfDue();
    await store.checkIfDue();
    expect(checker).toHaveBeenCalledTimes(1);
    now += CHECK_INTERVAL_MS;
    await store.checkIfDue();
    expect(checker).toHaveBeenCalledTimes(2);
  });

  it("does not retry a failed background check until the interval has passed", async () => {
    let now = 1_000_000_000_000;
    const checker = vi.fn(() => Promise.reject(new Error("no update feed yet")));
    const store = new UpdaterStore(
      checker,
      async () => {},
      () => now,
      openedByUser,
    );
    await store.checkIfDue();
    await store.checkIfDue();
    await store.checkIfDue();
    expect(checker).toHaveBeenCalledTimes(1);
    now += CHECK_INTERVAL_MS;
    await store.checkIfDue();
    expect(checker).toHaveBeenCalledTimes(2);
  });

  it("starts the automatic check on the next tick, not inside the caller", async () => {
    vi.useFakeTimers();
    try {
      const checker = vi.fn(() => Promise.resolve(null));
      const store = new UpdaterStore(checker, async () => {}, Date.now, openedByUser);
      store.setAutomatic(true);
      // Nothing may run synchronously: the caller is a component effect that must not track it.
      expect(checker).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(0);
      expect(checker).toHaveBeenCalledTimes(1);
      store.setAutomatic(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("waits half a minute before the first check when the OS started SoundPush", async () => {
    vi.useFakeTimers();
    try {
      const checker = vi.fn(() => Promise.resolve(null));
      const autostarted = async () => ({ autostarted: true, metered: false });
      const store = new UpdaterStore(checker, async () => {}, Date.now, autostarted);
      store.setAutomatic(true);
      await vi.advanceTimersByTimeAsync(0);
      // Signing in is busy enough without SoundPush reaching for the network (plan §24).
      expect(checker).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(AUTOSTART_DELAY_MS - 1);
      expect(checker).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(1);
      expect(checker).toHaveBeenCalledTimes(1);
      store.setAutomatic(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("drops the delayed first check when automatic checks are switched off meanwhile", async () => {
    vi.useFakeTimers();
    try {
      const checker = vi.fn(() => Promise.resolve(null));
      const autostarted = async () => ({ autostarted: true, metered: false });
      const store = new UpdaterStore(checker, async () => {}, Date.now, autostarted);
      store.setAutomatic(true);
      await vi.advanceTimersByTimeAsync(0);
      store.setAutomatic(false);
      await vi.advanceTimersByTimeAsync(AUTOSTART_DELAY_MS * 2);
      expect(checker).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("leaves a metered connection alone, and does not count the skip as a check", async () => {
    const now = 1_000_000_000_000;
    let metered = true;
    const checker = vi.fn(() => Promise.resolve(null));
    const store = new UpdaterStore(
      checker,
      async () => {},
      () => now,
      async () => ({
        autostarted: false,
        metered,
      }),
    );
    await store.checkIfDue();
    await store.checkIfDue();
    expect(checker).not.toHaveBeenCalled();
    // Back on Wi-Fi: the check that was skipped happens at once, without waiting a day.
    metered = false;
    await store.checkIfDue();
    expect(checker).toHaveBeenCalledTimes(1);
  });

  it("still checks on a metered connection when the user asks", async () => {
    const checker = vi.fn(() => Promise.resolve(null));
    const store = new UpdaterStore(
      checker,
      async () => {},
      Date.now,
      async () => ({
        autostarted: true,
        metered: true,
      }),
    );
    await store.check(true);
    expect(checker).toHaveBeenCalledTimes(1);
    expect(store.status).toBe("upToDate");
  });

  it("checks once per hour of polling at most, however often the effect re-runs", async () => {
    // The runaway-check bug: setAutomatic used to leave its timers behind.
    vi.useFakeTimers();
    try {
      const checker = vi.fn(() => Promise.resolve(null));
      const store = new UpdaterStore(checker, async () => {}, Date.now, openedByUser);
      for (let i = 0; i < 50; i++) store.setAutomatic(true);
      await vi.advanceTimersByTimeAsync(45_000);
      expect(checker).toHaveBeenCalledTimes(1);
      store.setAutomatic(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("brings the banner back when a newer version appears", async () => {
    let version = "1.0.0";
    const store = new UpdaterStore(() => Promise.resolve(fakeUpdate({ version })));
    await store.check(false);
    store.dismiss();
    await store.check(false);
    expect(store.dismissed).toBe(true);
    version = "1.1.0";
    await store.check(false);
    expect(store.dismissed).toBe(false);
  });
});
