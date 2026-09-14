#!/usr/bin/env node
// Clippy gate: "no new warnings" until the workspace is warning-free, then plain `-D warnings`.
//
//   cargo clippy --workspace --all-targets --message-format=json | node tools/ci/clippy-gate.mjs .github/clippy-allowed.txt
//
// The allowlist has one `<lint> <file>` per line (e.g. `clippy::result_unit_err sound-push-core/sp-engine/src/x.rs`).
// Any other warning fails the job with clippy's own message. An allowlisted warning that no longer occurs is reported
// so its line can be deleted; with an empty or missing list every warning fails, exactly like `-D warnings`.
import { existsSync, readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

export function parseAllowlist(text) {
  return new Set(
    text
      .split(/\r?\n/)
      .map((line) => line.replace(/#.*/, "").trim())
      .filter(Boolean)
      .map((line) => line.split(/\s+/).slice(0, 2).join(" ")),
  );
}

/** Warnings in cargo's JSON messages as { key: "<lint> <file>", rendered }. */
export function warningsFrom(lines) {
  const warnings = new Map();
  for (const line of lines) {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      continue;
    }
    if (message.reason !== "compiler-message") continue;
    const { level, code, spans = [], rendered } = message.message ?? {};
    if (level !== "warning" || !code) continue;
    const span = spans.find((s) => s.is_primary) ?? spans[0];
    if (!span) continue;
    const key = `${code.code} ${span.file_name.replaceAll("\\", "/")}`;
    if (!warnings.has(key)) warnings.set(key, rendered ?? key);
  }
  return warnings;
}

export function gate(warnings, allowed) {
  const fresh = [...warnings.keys()].filter((key) => !allowed.has(key));
  const fixed = [...allowed].filter((key) => !warnings.has(key));
  return { fresh, fixed };
}

async function main([allowlistPath]) {
  const allowed = parseAllowlist(allowlistPath && existsSync(allowlistPath) ? readFileSync(allowlistPath, "utf8") : "");
  const lines = [];
  for await (const line of createInterface({ input: process.stdin })) lines.push(line);
  const warnings = warningsFrom(lines);
  const { fresh, fixed } = gate(warnings, allowed);
  for (const key of fixed) {
    console.log(`::notice title=Clippy::Fixed: "${key}" no longer warns; delete it from ${allowlistPath}.`);
  }
  if (fresh.length === 0) {
    console.log(`Clippy: no new warnings (${warnings.size} allowlisted).`);
    return;
  }
  for (const key of fresh) {
    console.log(warnings.get(key));
    console.log(`::error title=Clippy::New warning ${key}`);
  }
  process.exit(1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main(process.argv.slice(2));
}
