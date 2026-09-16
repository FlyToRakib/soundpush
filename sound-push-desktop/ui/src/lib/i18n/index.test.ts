import { describe, expect, it } from "vitest";
import en from "./en.json";
import {
  PSEUDO_LONG,
  PSEUDO_RTL,
  dictionaries,
  formatElapsed,
  isRtl,
  pseudoLocalize,
  resolveLanguage,
  setLanguage,
  t,
  tp,
} from "./index";

describe("formatElapsed", () => {
  it("formats minutes and seconds", () => {
    expect(formatElapsed(0)).toBe("0:00");
    expect(formatElapsed(65)).toBe("1:05");
    expect(formatElapsed(599)).toBe("9:59");
  });

  it("adds hours when needed", () => {
    expect(formatElapsed(3600)).toBe("1:00:00");
    expect(formatElapsed(3725)).toBe("1:02:05");
  });
});

describe("t", () => {
  it("substitutes arguments", () => {
    expect(t("route.receiveSystemAudio", "Pixel")).toBe("Pixel audio → this computer");
  });

  it("falls back to the key for unknown strings", () => {
    expect(t("does.not.exist")).toBe("does.not.exist");
  });

  it("has a translation for every error key the engine emits", () => {
    const keys = [
      "error.device.notFound",
      "error.network.unreachable",
      "error.network.blocked",
      "error.network.firewall",
      "error.hotkey.invalid",
      "error.hotkey.duplicate",
      "error.hotkey.unavailable",
      "error.security.notPaired",
      "error.security.pairingRejected",
      "error.security.pairingExpired",
      "error.security.revoked",
      "error.compat.version",
      "error.permission.peerDenied",
      "error.permission.mic",
      "error.audio.device",
      "error.audio.loopbackUnsupported",
      "error.audio.virtualMicMissing",
      "error.route.notFound",
      "error.route.tooManyReceivers",
      "error.input.invalid",
      "error.storage",
      "error.engine.stopped",
      "error.internal",
    ];
    for (const key of keys) expect(t(key)).not.toBe(key);
  });
});

describe("plurals", () => {
  it("picks the plural form for the count", () => {
    setLanguage("en");
    expect(tp("devices.pairedCount", 1)).toBe("1 paired device");
    expect(tp("devices.pairedCount", 3)).toBe("3 paired devices");
    expect(tp("devices.pairedCount", 0)).toBe("0 paired devices");
  });

  it("formats the count for the language", () => {
    setLanguage("en");
    expect(tp("devices.pairedCount", 1200)).toBe("1,200 paired devices");
  });
});

describe("languages", () => {
  it("falls back from region to base language to English", () => {
    expect(resolveLanguage("en-GB")).toBe("en");
    expect(resolveLanguage("xx")).toBe("en");
    expect(resolveLanguage("system", ["xx-YY", "en-US"])).toBe("en");
  });

  it("knows right-to-left languages", () => {
    expect(isRtl("ar")).toBe(true);
    expect(isRtl("he-IL")).toBe(true);
    expect(isRtl(PSEUDO_RTL)).toBe(true);
    expect(isRtl("en")).toBe(false);
  });

  it("uses pseudo-locales for layout tests", () => {
    expect(setLanguage(PSEUDO_LONG)).toBe(PSEUDO_LONG);
    expect(t("nav.home")).not.toBe("Home");
    expect(t("route.receiveSystemAudio", "Pixel")).toContain("Pixel");
    setLanguage("en");
    expect(t("nav.home")).toBe("Home");
  });
});

describe("pseudoLocalize", () => {
  it("keeps placeholders and makes text longer", () => {
    const out = pseudoLocalize("Streaming to {0}");
    expect(out).toContain("{0}");
    expect(out.length).toBeGreaterThan("Streaming to {0}".length * 1.3);
    expect(out).not.toContain("Streaming");
  });

  it("marks right-to-left text", () => {
    expect(pseudoLocalize("Home", true).startsWith("‫")).toBe(true);
  });
});

describe("locale files", () => {
  const placeholders = (s: string) => [...s.matchAll(/\{\d+\}/g)].map((m) => m[0]).sort();

  it("only contain keys that exist in English, with the same placeholders", () => {
    const source = en as Record<string, string>;
    for (const [tag, dict] of Object.entries(dictionaries)) {
      if (tag === "en") continue;
      for (const [key, value] of Object.entries(dict)) {
        const base = key.replace(/_(zero|one|two|few|many|other)$/, "");
        const english = source[key] ?? source[`${base}_other`];
        expect(english, `${tag}: unknown key ${key}`).toBeDefined();
        expect(placeholders(value), `${tag}: placeholders in ${key}`).toEqual(placeholders(english ?? ""));
      }
    }
  });

  it("has plain string values in English", () => {
    for (const value of Object.values(en)) expect(typeof value).toBe("string");
  });
});
