#!/usr/bin/env node
// Fills a release's version and SHA-256 hashes into the store manifest templates in packaging/.
//
// Usage: node tools/release/fill-manifests.mjs --version 0.2.0 --sums SHA256SUMS --out store-manifests
//          [--repo FlyToRakib/soundpush] [--date YYYY-MM-DD] [--version-code N] [--commit v0.2.0] [--strict]
//   SHA256SUMS is the file attached to the GitHub Release (`gh release download v0.2.0 -p SHA256SUMS`).
//   A manifest whose package is missing from SHA256SUMS is skipped with a warning (an error with --strict).
// Output layout matches each store's repository:
//   winget/manifests/s/SoundPush/SoundPush/<version>/   → PR to microsoft/winget-pkgs
//   homebrew/Casks/s/soundpush.rb                        → tap (or homebrew/homebrew-cask)
//   flatpak/net.soundpush.desktop.{yml,metainfo.xml}     → flathub/net.soundpush.desktop
//   fdroid/metadata/net.soundpush.android.yml            → fdroiddata MR
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const ROOT = fileURLToPath(new URL("../../", import.meta.url));
export const DOCS_URL = "https://flytorakib.github.io/soundpush";

export function outputs(version) {
  const winget = `winget/manifests/s/SoundPush/SoundPush/${version}`;
  return [
    { template: "packaging/winget/SoundPush.SoundPush.yaml", out: `${winget}/SoundPush.SoundPush.yaml` },
    { template: "packaging/winget/SoundPush.SoundPush.installer.yaml", out: `${winget}/SoundPush.SoundPush.installer.yaml` },
    { template: "packaging/winget/SoundPush.SoundPush.locale.en-US.yaml", out: `${winget}/SoundPush.SoundPush.locale.en-US.yaml` },
    { template: "packaging/homebrew/soundpush.rb", out: "homebrew/Casks/s/soundpush.rb" },
    { template: "packaging/flatpak/net.soundpush.desktop.yml", out: "flatpak/net.soundpush.desktop.yml" },
    { template: "packaging/flatpak/net.soundpush.desktop.metainfo.xml", out: "flatpak/net.soundpush.desktop.metainfo.xml" },
    { template: "packaging/fdroid/net.soundpush.android.yml", out: "fdroid/metadata/net.soundpush.android.yml" },
  ];
}

/** `sha256sum` output → Map of file name to lower-case hash. */
export function parseSums(text) {
  const sums = new Map();
  for (const line of text.split(/\r?\n/)) {
    const match = /^([0-9a-fA-F]{64})\s+\*?(.+)$/.exec(line.trim());
    if (match) sums.set(match[2].trim(), match[1].toLowerCase());
  }
  return sums;
}

/** Placeholder values for the package files of `version` that exist in `sums`. */
export function packageHashes(sums, version) {
  const files = {
    SHA256_WINDOWS_X64: `SoundPush_${version}_x64-setup.exe`,
    SHA256_WINDOWS_ARM64: `SoundPush_${version}_arm64-setup.exe`,
    SHA256_MACOS_DMG: `SoundPush_${version}_universal.dmg`,
    SHA256_LINUX_DEB: `SoundPush_${version}_amd64.deb`,
  };
  const values = {};
  for (const [key, file] of Object.entries(files)) {
    if (sums.has(file)) values[key] = sums.get(file);
  }
  return values;
}

/** Replaces @KEY@ placeholders; a quoted "@VERSION_CODE@" becomes a bare number. Reports unknown keys. */
export function render(template, values) {
  const missing = new Set();
  let text = template;
  if (values.VERSION_CODE !== undefined) text = text.replaceAll('"@VERSION_CODE@"', String(values.VERSION_CODE));
  text = text.replace(/@([A-Z0-9_]+)@/g, (placeholder, key) => {
    if (values[key] === undefined) {
      missing.add(key);
      return placeholder;
    }
    return String(values[key]);
  });
  return { text, missing: [...missing] };
}

function parseFlags(argv) {
  const flags = {};
  for (let i = 0; i < argv.length; i++) {
    const name = argv[i].replace(/^--/, "");
    if (name === "strict") flags.strict = true;
    else flags[name] = argv[++i];
  }
  return flags;
}

function androidVersionCode() {
  const gradle = readFileSync(join(ROOT, "sound-push-mobile/android/app/build.gradle.kts"), "utf8");
  return Number(/versionCode\s*=\s*(\d+)/.exec(gradle)?.[1]);
}

function main(argv) {
  const flags = parseFlags(argv);
  const version = flags.version?.replace(/^v/, "");
  if (!version || !flags.sums || !flags.out) {
    console.error("usage: fill-manifests.mjs --version <x.y.z> --sums <SHA256SUMS> --out <dir> [--repo owner/name] [--date YYYY-MM-DD] [--version-code N] [--commit ref] [--strict]");
    process.exit(2);
  }
  if (!existsSync(flags.sums)) throw new Error(`${flags.sums} not found`);
  const repoUrl = `https://github.com/${flags.repo ?? "FlyToRakib/soundpush"}`;
  const values = {
    VERSION: version,
    RELEASE_DATE: flags.date ?? new Date().toISOString().slice(0, 10),
    REPO_URL: repoUrl,
    DOCS_URL,
    DOWNLOAD_BASE: `${repoUrl}/releases/download/v${version}`,
    VERSION_CODE: Number(flags["version-code"] ?? androidVersionCode()),
    COMMIT: flags.commit ?? `v${version}`,
    ...packageHashes(parseSums(readFileSync(flags.sums, "utf8")), version),
  };
  let failed = false;
  for (const { template, out } of outputs(version)) {
    const { text, missing } = render(readFileSync(join(ROOT, template), "utf8"), values);
    if (missing.length > 0) {
      const level = flags.strict ? "error" : "warning";
      console.error(`::${level} title=Store manifests::Skipped ${out}: no value for ${missing.join(", ")}`);
      failed ||= Boolean(flags.strict);
      continue;
    }
    const path = join(flags.out, out);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, text);
    console.error(`wrote ${path}`);
  }
  if (failed) process.exit(1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(`::error title=Store manifests::${e.message}`);
    process.exit(1);
  }
}
