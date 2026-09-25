#!/usr/bin/env node
// Refuses a commit that carries signing material. On 25 September 2026 a zip of the maintainer's
// `.soundpush` folder — both private signing keys and their plaintext passwords — was committed and
// pushed to this public repository; the keys had to be replaced. Nothing like it gets in again.
//
// Usage: node tools/ci/no-secrets.mjs [file...]   (no arguments: every tracked file)
import { execFileSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";

/** Paths that may never be committed, whatever they contain. */
const FORBIDDEN_PATHS = [
  { pattern: /(^|\/)\.soundpush(\/|\.|$)/i, why: "the maintainer's signing folder" },
  { pattern: /\.(jks|keystore|p12|pfx)$/i, why: "a keystore", unless: /(^|\/)debug\.keystore$/ },
  { pattern: /(^|\/)[^/]*(release|signing|updater)[^/]*\.(password|key|base64)$/i, why: "a signing key or its password" },
];

/** Contents that may never be committed, in any file. */
const FORBIDDEN_CONTENT = [
  { pattern: /-----BEGIN (RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----/, why: "a private key" },
  { pattern: /untrusted comment: (minisign|rsign) encrypted secret key/i, why: "the updater signing key" },
  { pattern: /\b(gh[pousr]_[A-Za-z0-9]{16,}|AKIA[0-9A-Z]{16})\b/, why: "an access token" },
];

/** A zip (or other archive) is opaque here, so look at what its entry names say it holds. */
function archiveEntries(bytes) {
  const names = [];
  const header = Buffer.from("PK\x03\x04");
  for (let at = bytes.indexOf(header); at >= 0; at = bytes.indexOf(header, at + 4)) {
    const nameLength = bytes.readUInt16LE(at + 26);
    names.push(bytes.slice(at + 30, at + 30 + nameLength).toString("latin1"));
  }
  return names;
}

const files = process.argv.slice(2).length
  ? process.argv.slice(2)
  : execFileSync("git", ["ls-files"], { encoding: "utf8" }).split("\n").filter(Boolean);

const found = [];
for (const file of files) {
  for (const { pattern, why, unless } of FORBIDDEN_PATHS) {
    if (pattern.test(file) && !unless?.test(file)) found.push(`${file}: looks like ${why}`);
  }
  let bytes;
  try {
    if (statSync(file).size > 8 * 1024 * 1024) continue;
    bytes = readFileSync(file);
  } catch {
    continue; // deleted or unreadable: nothing to check
  }
  const text = bytes.toString("latin1");
  for (const { pattern, why } of FORBIDDEN_CONTENT) {
    if (pattern.test(text)) found.push(`${file}: contains ${why}`);
  }
  for (const entry of archiveEntries(bytes)) {
    for (const { pattern, why, unless } of FORBIDDEN_PATHS) {
      if (pattern.test(entry) && !unless?.test(entry)) found.push(`${file}: archive holds ${entry} — ${why}`);
    }
  }
}

if (found.length) {
  for (const line of found) console.error(`::error title=Secret in the repository::${line}`);
  console.error(
    "\nThese must never be committed. Keep signing material outside the repository (see docs/release-signing.md);\n" +
      "if it was pushed, treat the key as public and replace it.",
  );
  process.exit(1);
}
console.log(`no signing material in ${files.length} files`);
