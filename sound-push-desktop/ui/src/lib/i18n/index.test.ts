import { describe, expect, it } from "vitest";
import { formatElapsed, t } from "./index";

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
      "error.input.invalid",
      "error.storage",
      "error.engine.stopped",
      "error.internal",
    ];
    for (const key of keys) expect(t(key)).not.toBe(key);
  });
});
