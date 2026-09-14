// Connection details (plan §23.3 "IP addresses, fingerprints, and raw stats live in Connection
// details", §28.2): how a device is reached, where the latency comes from, and a short history.
import type { PeerView, RouteStats } from "./engine/types";

/** usb: adb forwarding (TLS over TCP on loopback); usbTethering: the phone's USB network. */
export type PathKind = "usb" | "usbTethering" | "loopback" | "linkLocal" | "local" | "internet";

export interface ConnectionPath {
  transport: "" | "quic" | "tcp";
  family: 4 | 6 | null;
  kind: PathKind | null;
}

/** Host part of "192.168.1.20:47650", "[fe80::1%3]:47650" or a bare address. */
export function hostOf(address: string): string | null {
  const a = address.trim();
  if (!a) return null;
  if (a.startsWith("[")) {
    const end = a.indexOf("]");
    return end > 1 ? a.slice(1, end) : null;
  }
  const colons = a.split(":").length - 1;
  return colons === 1 ? a.slice(0, a.indexOf(":")) : a;
}

function ipv4(host: string): [number, number, number, number] | null {
  const parts = host.split(".");
  if (parts.length !== 4 || !parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255)) return null;
  return parts.map(Number) as [number, number, number, number];
}

/** Address family and scope of an IP address; null for anything else. */
export function classify(host: string): { family: 4 | 6; kind: "loopback" | "linkLocal" | "local" | "internet" } | null {
  const v4 = ipv4(host);
  if (v4) {
    const [a, b] = v4;
    if (a === 127) return { family: 4, kind: "loopback" };
    if (a === 169 && b === 254) return { family: 4, kind: "linkLocal" };
    // Private ranges, and 100.64/10 which phone hotspots and carriers use.
    const local = a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 100 && b >= 64 && b <= 127);
    return { family: 4, kind: local ? "local" : "internet" };
  }
  const h = host.split("%")[0]!.toLowerCase();
  if (!h.includes(":") || !/^[0-9a-f:.]+$/.test(h)) return null;
  if (h === "::1") return { family: 6, kind: "loopback" };
  // IPv4-mapped addresses from dual-stack sockets are IPv4 connections.
  if (h.startsWith("::ffff:") && ipv4(h.slice(7))) return classify(h.slice(7));
  const first = h.startsWith("::") ? 0 : parseInt(h.split(":")[0]!, 16);
  if ((first & 0xffc0) === 0xfe80) return { family: 6, kind: "linkLocal" };
  if ((first & 0xfe00) === 0xfc00) return { family: 6, kind: "local" };
  return { family: 6, kind: "internet" };
}

/** How this computer reaches `peer`. `tethered`: the path runs over the phone's USB tethering. */
export function connectionPath(peer: PeerView | undefined, tethered = false): ConnectionPath {
  if (!peer || peer.connection !== "connected") return { transport: "", family: null, kind: null };
  const host = hostOf(peer.remoteAddress);
  const c = host ? classify(host) : null;
  let kind: PathKind | null = c?.kind ?? null;
  if (peer.transport === "tcp" && kind === "loopback") kind = "usb";
  else if (tethered) kind = "usbTethering";
  return { transport: peer.transport, family: c?.family ?? null, kind };
}

export type StageId = "capture" | "encode" | "network" | "buffer" | "output";

/** The parts of the estimated latency, in pipeline order. Unknown (zero) parts are left out. */
export function latencyStages(stats: RouteStats): { id: StageId; ms: number }[] {
  const stages: { id: StageId; ms: number }[] = [
    { id: "capture", ms: stats.captureMs },
    { id: "encode", ms: stats.encodeMs },
    { id: "network", ms: stats.networkMs },
    { id: "buffer", ms: stats.bufferMs },
    { id: "output", ms: stats.outputMs },
  ];
  return stages.filter((s) => s.ms > 0);
}

export interface Sample {
  /** Milliseconds since the epoch. */
  at: number;
  latencyMs: number;
  lossPct: number;
}

/** How far back the chart goes. */
export const HISTORY_MS = 5 * 60_000;
const MIN_SPACING_MS = 1000;

/** `samples` plus `sample`, at most one per second and only the last HISTORY_MS. Unchanged input is returned as is. */
export function appendSample(samples: readonly Sample[], sample: Sample): readonly Sample[] {
  const last = samples[samples.length - 1];
  if (last && sample.at - last.at < MIN_SPACING_MS) return samples;
  const cutoff = sample.at - HISTORY_MS;
  return [...samples.filter((s) => s.at >= cutoff), sample];
}

export interface SampleSummary {
  minMs: number;
  avgMs: number;
  maxMs: number;
  maxLossPct: number;
}

export function summarize(samples: readonly Sample[]): SampleSummary | null {
  if (samples.length === 0) return null;
  let min = Infinity;
  let max = 0;
  let sum = 0;
  let loss = 0;
  for (const s of samples) {
    min = Math.min(min, s.latencyMs);
    max = Math.max(max, s.latencyMs);
    sum += s.latencyMs;
    loss = Math.max(loss, s.lossPct);
  }
  return { minMs: min, avgMs: sum / samples.length, maxMs: max, maxLossPct: loss };
}
