// node --test tools/release/*.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it } from "node:test";
import { DOCS_URL, ROOT, outputs, packageHashes, parseSums, render } from "./fill-manifests.mjs";

const hash = (c) => c.repeat(64);

describe("fill-manifests", () => {
  it("parses sha256sum output, including binary markers", () => {
    const sums = parseSums(`${hash("A")}  SoundPush_1.0.0_x64-setup.exe\n${hash("b")} *SoundPush_1.0.0_universal.dmg\nnot a line\n`);
    assert.equal(sums.get("SoundPush_1.0.0_x64-setup.exe"), hash("a"));
    assert.equal(sums.get("SoundPush_1.0.0_universal.dmg"), hash("b"));
    assert.equal(sums.size, 2);
  });

  it("maps only the packages that exist", () => {
    const sums = parseSums(`${hash("c")}  SoundPush_1.0.0_amd64.deb\n`);
    assert.deepEqual(packageHashes(sums, "1.0.0"), { SHA256_LINUX_DEB: hash("c") });
  });

  it("replaces placeholders and reports missing ones", () => {
    const { text, missing } = render('v: "@VERSION@"\ncode: "@VERSION_CODE@"\nsha: "@SHA256_X@"\n', {
      VERSION: "1.0.0",
      VERSION_CODE: 7,
    });
    assert.equal(text, 'v: "1.0.0"\ncode: 7\nsha: "@SHA256_X@"\n');
    assert.deepEqual(missing, ["SHA256_X"]);
  });

  it("fills every template completely from a full release", () => {
    const version = "1.2.3";
    const sums = parseSums(
      ["x64-setup.exe", "arm64-setup.exe", "universal.dmg", "amd64.deb"]
        .map((suffix, i) => `${hash(String(i + 1))}  SoundPush_${version}_${suffix}`)
        .join("\n"),
    );
    const values = {
      VERSION: version,
      RELEASE_DATE: "2026-10-01",
      REPO_URL: "https://github.com/FlyToRakib/soundpush",
      DOCS_URL,
      DOWNLOAD_BASE: `https://github.com/FlyToRakib/soundpush/releases/download/v${version}`,
      VERSION_CODE: 3,
      COMMIT: `v${version}`,
      ...packageHashes(sums, version),
    };
    for (const { template } of outputs(version)) {
      const { text, missing } = render(readFileSync(join(ROOT, template), "utf8"), values);
      assert.deepEqual(missing, [], template);
      assert.ok(text.includes(version) || template.endsWith("fdroid/net.soundpush.android.yml"), template);
    }
  });
});
