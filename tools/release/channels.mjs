#!/usr/bin/env node
// Desktop update channels and staged rollout (docs/soundpush-final.md §32, docs/release-signing.md).
//
// Every published release with a signed latest.json is a candidate:
//   stable.json  newest release without a pre-release suffix (v1.2.3), with a "rollout" share that follows
//                SCHEDULE from its publish time (10 % → 50 % after 24 h → 100 % after 72 h),
//   beta.json    newest release of any kind (v1.3.0-beta.1 or v1.2.3), offered to every beta install at once.
// The files live on the rolling "updates" release; .github/workflows/update-channels.yml runs this script.
//
//   node channels.mjs pick <releases.json>
//       prints stable=<tag> and beta=<tag> (GitHub Actions output format)
//   node channels.mjs write --releases <releases.json> --out <dir> [--stable-manifest <latest.json>]
//       [--beta-manifest <latest.json>] [--current-stable <stable.json>] [--rollout <0-100|hold|resume>] [--now <iso>]
//       writes <dir>/stable.json and/or <dir>/beta.json
//
// --rollout pins the stable share (a number, or "hold" to stop where it is) until "resume" returns to the schedule.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** [hours after publishing, share of installs in %]. */
export const SCHEDULE = [
  [0, 10],
  [24, 50],
  [72, 100],
];

const TAG = /^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

/** SemVer precedence of two tags or versions ("v1.2.3", "1.3.0-beta.2"). */
export function compareVersions(a, b) {
  const pa = TAG.exec(`v${a.replace(/^v/, "")}`);
  const pb = TAG.exec(`v${b.replace(/^v/, "")}`);
  if (!pa || !pb) throw new Error(`not a version: ${pa ? b : a}`);
  for (let i = 1; i <= 3; i++) {
    const d = Number(pa[i]) - Number(pb[i]);
    if (d !== 0) return Math.sign(d);
  }
  if (!pa[4] || !pb[4]) return pa[4] ? -1 : pb[4] ? 1 : 0;
  const xa = pa[4].split(".");
  const xb = pb[4].split(".");
  for (let i = 0; i < Math.max(xa.length, xb.length); i++) {
    if (xa[i] === undefined) return -1;
    if (xb[i] === undefined) return 1;
    const na = /^\d+$/.test(xa[i]);
    const nb = /^\d+$/.test(xb[i]);
    if (na && nb && Number(xa[i]) !== Number(xb[i])) return Math.sign(Number(xa[i]) - Number(xb[i]));
    if (na !== nb) return na ? -1 : 1;
    if (!na && xa[i] !== xb[i]) return xa[i] < xb[i] ? -1 : 1;
  }
  return 0;
}

/** Newest stable and newest overall published release that carries an updater manifest. */
export function pickReleases(releases) {
  const candidates = releases.filter(
    (r) => !r.draft && TAG.test(r.tag_name) && (r.assets ?? []).some((a) => a.name === "latest.json"),
  );
  const newest = (list) => list.reduce((best, r) => (!best || compareVersions(r.tag_name, best.tag_name) > 0 ? r : best), null);
  return {
    stable: newest(candidates.filter((r) => !TAG.exec(r.tag_name)[4] && !r.prerelease)),
    beta: newest(candidates),
  };
}

/** Scheduled share for a release published at `publishedAt`. */
export function scheduledRollout(publishedAt, now) {
  const hours = (new Date(now).getTime() - new Date(publishedAt).getTime()) / 3_600_000;
  let share = SCHEDULE[0][1];
  for (const [after, percent] of SCHEDULE) if (hours >= after) share = percent;
  return share;
}

/**
 * Stable rollout state for `release`.
 * `current` is the stable.json published before (or null); `override` is "", a number string, "hold" or "resume".
 */
export function stableRollout({ release, current, override = "", now }) {
  const version = release.tag_name.replace(/^v/, "");
  const scheduled = scheduledRollout(release.published_at, now);
  const same = current && current.version === version && typeof current.rollout === "number";
  const value = String(override).trim();
  if (value === "hold") return { rollout: same ? current.rollout : scheduled, rolloutHold: true };
  if (value === "resume") return { rollout: same ? Math.max(scheduled, current.rollout) : scheduled, rolloutHold: false };
  if (value !== "") {
    const pinned = Number(value);
    if (!Number.isInteger(pinned) || pinned < 0 || pinned > 100) throw new Error(`rollout must be 0-100, hold or resume: ${value}`);
    return { rollout: pinned, rolloutHold: true };
  }
  if (same && current.rolloutHold) return { rollout: current.rollout, rolloutHold: true };
  // Never shrink a running rollout on its own (a later schedule change or a clock skew).
  return { rollout: same ? Math.max(scheduled, current.rollout) : scheduled, rolloutHold: false };
}

export function stableManifest(latest, state) {
  return { ...latest, channel: "stable", rollout: state.rollout, rolloutHold: state.rolloutHold };
}

export function betaManifest(latest) {
  const { rolloutHold: _hold, ...rest } = latest;
  return { ...rest, channel: "beta", rollout: 100 };
}

function readJson(path) {
  return path && existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : null;
}

function parseFlags(argv) {
  const flags = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i].startsWith("--")) throw new Error(`unexpected argument ${argv[i]}`);
    flags[argv[i].slice(2)] = argv[i + 1] ?? "";
  }
  return flags;
}

function main([command, ...args]) {
  if (command === "pick") {
    const { stable, beta } = pickReleases(readJson(args[0]) ?? []);
    process.stdout.write(`stable=${stable?.tag_name ?? ""}\nbeta=${beta?.tag_name ?? ""}\n`);
    return;
  }
  if (command === "write") {
    const flags = parseFlags(args);
    if (!flags.releases || !flags.out) throw new Error("write needs --releases and --out");
    const { stable, beta } = pickReleases(readJson(flags.releases) ?? []);
    const now = flags.now || new Date().toISOString();
    mkdirSync(flags.out, { recursive: true });
    const stableLatest = readJson(flags["stable-manifest"]);
    if (stable && stableLatest) {
      if (stableLatest.version !== stable.tag_name.replace(/^v/, "")) {
        throw new Error(`stable manifest is ${stableLatest.version}, expected ${stable.tag_name}`);
      }
      const state = stableRollout({ release: stable, current: readJson(flags["current-stable"]), override: flags.rollout, now });
      writeFileSync(join(flags.out, "stable.json"), `${JSON.stringify(stableManifest(stableLatest, state), null, 2)}\n`);
      console.error(`stable.json: ${stable.tag_name}, rollout ${state.rollout} %${state.rolloutHold ? " (held)" : ""}`);
    }
    const betaLatest = readJson(flags["beta-manifest"]);
    if (beta && betaLatest) {
      writeFileSync(join(flags.out, "beta.json"), `${JSON.stringify(betaManifest(betaLatest), null, 2)}\n`);
      console.error(`beta.json: ${beta.tag_name}`);
    }
    return;
  }
  console.error("usage: channels.mjs pick <releases.json> | write --releases <file> --out <dir> [...]");
  process.exit(2);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(`::error title=Update channels::${e.message}`);
    process.exit(1);
  }
}
