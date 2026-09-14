#!/usr/bin/env node
// Writes the Tauri updater manifest (latest.json) from signed update bundles in a folder.
// Usage: node tools/release/latest-json.mjs <version> <dir> <download-base-url> [notes-file] [--channel stable|beta]
//   e.g. latest-json.mjs 0.2.0 dist https://github.com/FlyToRakib/soundpush/releases/download/v0.2.0 notes.md
// Each bundle needs its minisign signature next to it (<file>.sig), produced by `tauri build` when
// TAURI_SIGNING_PRIVATE_KEY is set. The app verifies the signature against the public key in tauri.conf.json.
// The per-channel manifests (stable.json with the staged-rollout share, beta.json) are derived from this file by
// channels.mjs once the release is published.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** Updater platform keys and the bundle each one installs. */
export const PLATFORMS = [
  { keys: ["windows-x86_64"], pattern: /_x64-setup\.exe$/ },
  { keys: ["windows-aarch64"], pattern: /_arm64-setup\.exe$/ },
  // One universal app serves both Mac architectures.
  { keys: ["darwin-aarch64", "darwin-x86_64"], pattern: /_universal\.app\.tar\.gz$/ },
  // Only the AppImage can update itself; deb/rpm installs update through the package manager or a new download.
  { keys: ["linux-x86_64"], pattern: /_amd64\.AppImage$/ },
];

export function buildManifest({ version, files, readSignature, baseUrl, notes, channel, date = new Date() }) {
  const platforms = {};
  for (const { keys, pattern } of PLATFORMS) {
    const bundle = files.find((f) => pattern.test(f) && files.includes(`${f}.sig`));
    if (!bundle) continue;
    const entry = { signature: readSignature(`${bundle}.sig`).trim(), url: `${baseUrl}/${encodeURIComponent(bundle)}` };
    for (const key of keys) platforms[key] = entry;
  }
  const manifest = { version, notes, pub_date: date.toISOString(), platforms };
  if (channel) manifest.channel = channel;
  return manifest;
}

function main(argv) {
  const positional = [];
  let channel;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--channel") channel = argv[++i];
    else positional.push(argv[i]);
  }
  const [version, dir, baseUrl, notesFile] = positional;
  if (!version || !dir || !baseUrl || (channel && !["stable", "beta"].includes(channel))) {
    console.error("usage: latest-json.mjs <version> <dir> <download-base-url> [notes-file] [--channel stable|beta]");
    process.exit(2);
  }
  const files = readdirSync(dir);
  const notes = notesFile && existsSync(notesFile) ? readFileSync(notesFile, "utf8").trim() : "";
  const manifest = buildManifest({
    version: version.replace(/^v/, ""),
    files,
    readSignature: (name) => readFileSync(join(dir, name), "utf8"),
    baseUrl: baseUrl.replace(/\/$/, ""),
    notes,
    channel,
  });
  const found = Object.keys(manifest.platforms);
  if (found.length === 0) {
    console.error("::error title=Updater::No signed update bundles (*.sig) found; cannot write latest.json.");
    process.exit(1);
  }
  for (const { keys, pattern } of PLATFORMS) {
    if (!keys.some((k) => found.includes(k))) {
      console.error(`::warning title=Updater::No signed bundle matching ${pattern} — ${keys.join(", ")} will not auto-update.`);
    }
  }
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2));
}
