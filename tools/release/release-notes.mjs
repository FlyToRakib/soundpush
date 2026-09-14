#!/usr/bin/env node
// Prints the CHANGELOG.md section for a version, for the GitHub Release notes.
// Usage: node tools/release/release-notes.mjs <version> [path/to/CHANGELOG.md]
// Falls back to the "Unreleased" section (with a GitHub Actions warning) when the version has no section yet.
import { readFileSync } from "node:fs";

/** Body of the `## [heading]` section (without the heading), or null. */
export function section(changelog, heading) {
  const lines = changelog.split(/\r?\n/);
  const escaped = heading.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const start = lines.findIndex((l) => new RegExp(`^## \\[${escaped}\\]`, "i").test(l));
  if (start < 0) return null;
  const body = [];
  for (const line of lines.slice(start + 1)) {
    if (/^## /.test(line) || /^\[[^\]]+\]:\s/.test(line)) break;
    body.push(line);
  }
  return body.join("\n").trim();
}

const [version, file = "CHANGELOG.md"] = process.argv.slice(2);
if (!version) {
  console.error("usage: release-notes.mjs <version> [CHANGELOG.md]");
  process.exit(2);
}
const text = readFileSync(file, "utf8");
let notes = section(text, version.replace(/^v/, ""));
if (notes === null) {
  notes = section(text, "Unreleased");
  console.error(
    `::warning title=Release notes::CHANGELOG.md has no section for ${version}; using "Unreleased". Rename it before publishing.`,
  );
}
if (!notes) {
  console.error(`::error title=Release notes::CHANGELOG.md has no notes for ${version}.`);
  process.exit(1);
}
process.stdout.write(`${notes}\n`);
