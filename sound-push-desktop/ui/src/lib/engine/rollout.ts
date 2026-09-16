// Staged rollout of desktop updates (docs/release-signing.md, "Update channels and staged rollout").
//
// A manifest may carry `"rollout": 0–100`, the share of installs offered the update by background checks.
// Each install has a fixed bucket per version, so raising the share (10 → 50 → 100) only adds installs.
// A check the user starts still gets the update, unless the rollout is halted (0).

/** Share of installs (0–100) that should see this manifest's update. Missing or invalid means everyone. */
export function rolloutPercent(rawJson: Record<string, unknown>): number {
  const value = rawJson.rollout;
  if (typeof value !== "number" || !Number.isFinite(value)) return 100;
  return Math.min(100, Math.max(0, value));
}

/** This install's bucket (0–99) for `version`: always the same for the pair, spread evenly across installs. */
export function rolloutBucket(installId: string, version: string): number {
  // FNV-1a, 32 bit.
  let hash = 0x811c9dc5;
  for (const char of `${installId}:${version}`) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash % 100;
}

/** Whether to offer the update in `rawJson` to this install. */
export function offerUpdate(rawJson: Record<string, unknown>, installId: string, version: string, manual: boolean): boolean {
  const percent = rolloutPercent(rawJson);
  if (percent <= 0) return false;
  if (manual || percent >= 100) return true;
  return rolloutBucket(installId, version) < percent;
}
