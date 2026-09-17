// node --test tools/release/
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { betaManifest, compareVersions, pickReleases, scheduledRollout, stableManifest, stableRollout } from "./channels.mjs";

const release = (tag_name, extra = {}) => ({
  tag_name,
  draft: false,
  prerelease: tag_name.includes("-"),
  published_at: "2026-10-01T00:00:00Z",
  assets: [{ name: "latest.json" }],
  ...extra,
});

describe("compareVersions", () => {
  it("orders SemVer including pre-releases", () => {
    const sorted = ["v1.0.0", "v0.9.0", "v1.0.0-beta.2", "v1.0.0-beta.10", "v1.0.0-alpha", "v1.1.0-beta.1"].sort(compareVersions);
    assert.deepEqual(sorted, ["v0.9.0", "v1.0.0-alpha", "v1.0.0-beta.2", "v1.0.0-beta.10", "v1.0.0", "v1.1.0-beta.1"]);
  });
});

describe("pickReleases", () => {
  it("picks the newest stable and the newest overall with a manifest", () => {
    const picked = pickReleases([
      release("v0.2.0"),
      release("v0.3.0-beta.1"),
      release("v0.2.1", { draft: true }),
      release("v0.2.2", { assets: [] }),
      release("updates", { prerelease: true }),
      release("v0.1.0"),
    ]);
    assert.equal(picked.stable.tag_name, "v0.2.0");
    assert.equal(picked.beta.tag_name, "v0.3.0-beta.1");
  });

  it("gives beta the stable release when it is newer", () => {
    const picked = pickReleases([release("v0.3.0-beta.1"), release("v0.3.0")]);
    assert.equal(picked.beta.tag_name, "v0.3.0");
  });

  it("keeps a 0.x preview on the stable channel although GitHub marks it pre-release", () => {
    const picked = pickReleases([release("v0.1.0", { prerelease: true }), release("v0.2.0-beta.1")]);
    assert.equal(picked.stable.tag_name, "v0.1.0");
    assert.equal(picked.beta.tag_name, "v0.2.0-beta.1");
  });

  it("takes a version from 1.0 on off the stable channel when it is marked pre-release", () => {
    const picked = pickReleases([release("v1.0.0"), release("v1.0.1", { prerelease: true })]);
    assert.equal(picked.stable.tag_name, "v1.0.0");
  });

  it("handles no releases", () => {
    assert.deepEqual(pickReleases([]), { stable: null, beta: null });
  });
});

describe("staged rollout", () => {
  const r = release("v0.2.0");
  const at = (hours) => new Date(Date.parse(r.published_at) + hours * 3_600_000).toISOString();

  it("follows 10 → 50 → 100 over 72 hours", () => {
    assert.equal(scheduledRollout(r.published_at, at(1)), 10);
    assert.equal(scheduledRollout(r.published_at, at(25)), 50);
    assert.equal(scheduledRollout(r.published_at, at(72)), 100);
  });

  it("starts a new version on the schedule", () => {
    const current = { version: "0.1.0", rollout: 100, rolloutHold: true };
    assert.deepEqual(stableRollout({ release: r, current, now: at(2) }), { rollout: 10, rolloutHold: false });
  });

  it("keeps a held rollout until resumed", () => {
    const held = stableRollout({ release: r, current: { version: "0.2.0", rollout: 10 }, override: "hold", now: at(30) });
    assert.deepEqual(held, { rollout: 10, rolloutHold: true });
    const later = stableRollout({ release: r, current: { version: "0.2.0", ...held }, now: at(80) });
    assert.deepEqual(later, { rollout: 10, rolloutHold: true });
    const resumed = stableRollout({ release: r, current: { version: "0.2.0", ...held }, override: "resume", now: at(80) });
    assert.deepEqual(resumed, { rollout: 100, rolloutHold: false });
  });

  it("pins a manual share, including 0 to halt", () => {
    assert.deepEqual(stableRollout({ release: r, current: null, override: "0", now: at(80) }), { rollout: 0, rolloutHold: true });
    assert.throws(() => stableRollout({ release: r, current: null, override: "150", now: at(1) }));
  });

  it("never shrinks a running rollout by itself", () => {
    const state = stableRollout({ release: r, current: { version: "0.2.0", rollout: 50, rolloutHold: false }, now: at(1) });
    assert.deepEqual(state, { rollout: 50, rolloutHold: false });
  });

  it("writes channel fields into the manifests", () => {
    const latest = { version: "0.2.0", notes: "", pub_date: r.published_at, platforms: {} };
    assert.deepEqual(stableManifest(latest, { rollout: 10, rolloutHold: false }), {
      ...latest,
      channel: "stable",
      rollout: 10,
      rolloutHold: false,
    });
    assert.equal(betaManifest({ ...latest, rollout: 10, rolloutHold: true }).rollout, 100);
    assert.equal("rolloutHold" in betaManifest({ ...latest, rolloutHold: true }), false);
  });
});
