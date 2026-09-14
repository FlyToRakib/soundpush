import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AvailableUpdate } from "../engine/updater";
import { CHECK_INTERVAL_MS, UpdaterStore } from "./updater.svelte";

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
    const store = new UpdaterStore(checker, async () => {}, () => now);
    await store.checkIfDue();
    await store.checkIfDue();
    expect(checker).toHaveBeenCalledTimes(1);
    now += CHECK_INTERVAL_MS;
    await store.checkIfDue();
    expect(checker).toHaveBeenCalledTimes(2);
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
