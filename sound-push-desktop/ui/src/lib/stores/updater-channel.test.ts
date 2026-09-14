import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AvailableUpdate, CheckOptions } from "../engine/updater";
import { UpdaterStore } from "./updater.svelte";

const nextTick = () => new Promise((resolve) => setTimeout(resolve, 0));

function update(version: string): AvailableUpdate {
  return { version, notes: "", download: async () => {}, install: async () => {} };
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

describe("UpdaterStore channels", () => {
  it("checks the configured channel with the install id", async () => {
    const checker = vi.fn((_: CheckOptions) => Promise.resolve(null));
    const store = new UpdaterStore(checker);
    store.configure("beta", "abc");
    await store.check(true);
    expect(checker).toHaveBeenCalledWith({ channel: "beta", installId: "abc", manual: true });
  });

  it("does not check when the channel is first set", async () => {
    const checker = vi.fn((_: CheckOptions) => Promise.resolve(null));
    const store = new UpdaterStore(checker);
    store.setAutomatic(true);
    await nextTick();
    checker.mockClear();
    store.configure("beta", "abc");
    await nextTick();
    expect(checker).not.toHaveBeenCalled();
    store.setAutomatic(false);
  });

  it("checks the new channel once, on the next tick, after a switch", async () => {
    const checker = vi.fn(({ channel }: CheckOptions) => Promise.resolve(channel === "beta" ? update("2.0.0-beta.1") : null));
    const store = new UpdaterStore(checker);
    store.configure("stable", "abc");
    store.setAutomatic(true);
    await nextTick();
    checker.mockClear();

    store.configure("beta", "abc");
    expect(checker).not.toHaveBeenCalled();
    await nextTick();
    await nextTick();
    expect(checker).toHaveBeenCalledOnce();
    expect(store.status).toBe("available");
    expect(store.version).toBe("2.0.0-beta.1");

    // Same channel again (settings re-emitted): nothing new.
    store.configure("beta", "abc");
    await nextTick();
    expect(checker).toHaveBeenCalledOnce();

    // Back to stable: the beta offer is dropped.
    store.configure("stable", "abc");
    expect(store.status).toBe("idle");
    expect(store.version).toBeNull();
    store.setAutomatic(false);
  });

  it("does not check on a switch while automatic checks are off", async () => {
    const checker = vi.fn((_: CheckOptions) => Promise.resolve(null));
    const store = new UpdaterStore(checker);
    store.configure("stable", "abc");
    store.configure("beta", "abc");
    await nextTick();
    expect(checker).not.toHaveBeenCalled();
  });
});
