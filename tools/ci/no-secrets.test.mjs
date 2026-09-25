import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const dir = mkdtempSync(join(tmpdir(), "no-secrets-"));
const run = (...files) => {
  try {
    execFileSync("node", ["tools/ci/no-secrets.mjs", ...files], { encoding: "utf8", stdio: "pipe" });
    return { failed: false, output: "" };
  } catch (error) {
    return { failed: true, output: `${error.stdout}${error.stderr}` };
  }
};

describe("no-secrets", () => {
  it("passes a plain file", () => {
    const file = join(dir, "notes.md");
    writeFileSync(file, "# Notes\nNothing secret here.\n");
    assert.equal(run(file).failed, false);
  });

  it("refuses a keystore by its name, but not the shared debug key", () => {
    assert.equal(run("secrets/soundpush-release.jks").failed, true);
    assert.equal(run("sound-push-mobile/android/debug.keystore").failed, false);
  });

  it("refuses a private key or token by its contents", () => {
    const file = join(dir, "config.txt");
    // Assembled at run time so this test file is not a finding itself.
    writeFileSync(file, `key: ${["-----BEGIN", "PRIVATE", "KEY-----"].join(" ")}\n`);
    const result = run(file);
    assert.equal(result.failed, true);
    assert.match(result.output, /a private key/);
  });

  it("looks inside an archive, which is how the keys were leaked", () => {
    const file = join(dir, "backup.zip");
    const entry = Buffer.from(".soundpush/updater/soundpush-updater.key");
    const header = Buffer.alloc(30);
    header.write("PK\u0003\u0004", "latin1");
    header.writeUInt16LE(entry.length, 26);
    writeFileSync(file, Buffer.concat([header, entry]));
    const result = run(file);
    assert.equal(result.failed, true);
    assert.match(result.output, /archive holds/);
  });
});
