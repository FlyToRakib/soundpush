import { describe, expect, it } from "vitest";
import { offerUpdate, rolloutBucket, rolloutPercent } from "./rollout";

const ids: string[] = Array.from({ length: 2000 }, (_, i) => i.toString(16).padStart(32, "0"));
const someId = "0123456789abcdef0123456789abcdef";

describe("staged rollout", () => {
  it("offers updates without a rollout field to everyone", () => {
    expect(rolloutPercent({})).toBe(100);
    expect(rolloutPercent({ rollout: "50" })).toBe(100);
    expect(offerUpdate({}, someId, "1.0.0", false)).toBe(true);
  });

  it("clamps the percentage", () => {
    expect(rolloutPercent({ rollout: 150 })).toBe(100);
    expect(rolloutPercent({ rollout: -5 })).toBe(0);
  });

  it("gives each install the same bucket for a version", () => {
    expect(rolloutBucket(someId, "1.2.0")).toBe(rolloutBucket(someId, "1.2.0"));
    for (const id of ids.slice(0, 50)) {
      const bucket = rolloutBucket(id, "1.2.0");
      expect(bucket).toBeGreaterThanOrEqual(0);
      expect(bucket).toBeLessThan(100);
    }
  });

  it("reaches roughly the requested share of installs", () => {
    const offered = ids.filter((id) => offerUpdate({ rollout: 10 }, id, "1.2.0", false)).length;
    expect(offered / ids.length).toBeGreaterThan(0.06);
    expect(offered / ids.length).toBeLessThan(0.14);
  });

  it("only adds installs when the share grows", () => {
    for (const id of ids) {
      if (offerUpdate({ rollout: 10 }, id, "1.2.0", false)) {
        expect(offerUpdate({ rollout: 50 }, id, "1.2.0", false)).toBe(true);
      }
    }
  });

  it("lets a user-started check through, but not a halted rollout", () => {
    const outside = ids.find((id) => !offerUpdate({ rollout: 10 }, id, "1.2.0", false)) ?? someId;
    expect(offerUpdate({ rollout: 10 }, outside, "1.2.0", false)).toBe(false);
    expect(offerUpdate({ rollout: 10 }, outside, "1.2.0", true)).toBe(true);
    expect(offerUpdate({ rollout: 0 }, outside, "1.2.0", true)).toBe(false);
  });
});
