import { describe, expect, it } from "vitest";
import { HISTORY_MS, appendSample, classify, connectionPath, hostOf, latencyStages, summarize, type Sample } from "./connection";
import type { PeerView, RouteStats } from "./engine/types";

const peer = (remoteAddress: string, transport: PeerView["transport"] = "quic"): PeerView =>
  ({ deviceId: "11", connection: "connected", remoteAddress, transport }) as PeerView;

describe("connection path", () => {
  it("reads hosts from socket addresses", () => {
    expect(hostOf("192.168.1.20:47650")).toBe("192.168.1.20");
    expect(hostOf("[fe80::1%3]:47650")).toBe("fe80::1%3");
    expect(hostOf("fd00::2")).toBe("fd00::2");
    expect(hostOf("")).toBeNull();
  });

  it("classifies address family and scope", () => {
    expect(classify("192.168.1.20")).toEqual({ family: 4, kind: "local" });
    expect(classify("172.20.10.2")).toEqual({ family: 4, kind: "local" });
    expect(classify("8.8.8.8")).toEqual({ family: 4, kind: "internet" });
    expect(classify("127.0.0.1")).toEqual({ family: 4, kind: "loopback" });
    expect(classify("169.254.3.4")).toEqual({ family: 4, kind: "linkLocal" });
    expect(classify("fe80::1c2b%12")).toEqual({ family: 6, kind: "linkLocal" });
    expect(classify("fd12:3456::1")).toEqual({ family: 6, kind: "local" });
    expect(classify("::ffff:10.0.0.2")).toEqual({ family: 4, kind: "local" });
    expect(classify("2001:db8::1")).toEqual({ family: 6, kind: "internet" });
    expect(classify("pixel.local")).toBeNull();
  });

  it("names USB (adb) and USB tethering paths", () => {
    expect(connectionPath(peer("127.0.0.1:50123", "tcp"))).toEqual({ transport: "tcp", family: 4, kind: "usb" });
    expect(connectionPath(peer("192.168.42.129:47650"), true).kind).toBe("usbTethering");
    expect(connectionPath({ ...peer("1.2.3.4:1"), connection: "reconnecting" })).toEqual({ transport: "", family: null, kind: null });
  });
});

describe("latency history", () => {
  const at = (sec: number, latencyMs = 40, lossPct = 0): Sample => ({ at: sec * 1000, latencyMs, lossPct });

  it("keeps one sample per second for five minutes", () => {
    let samples: readonly Sample[] = [];
    samples = appendSample(samples, at(0));
    const same = appendSample(samples, { ...at(0), at: 400 });
    expect(same).toBe(samples);
    for (let s = 1; s <= 400; s++) samples = appendSample(samples, at(s));
    expect(samples.length).toBe(HISTORY_MS / 1000 + 1);
    expect(samples[0]!.at).toBe((400 - HISTORY_MS / 1000) * 1000);
  });

  it("summarizes for the text equivalent", () => {
    expect(summarize([])).toBeNull();
    expect(summarize([at(0, 30, 0), at(1, 50, 2.5)])).toEqual({ minMs: 30, avgMs: 40, maxMs: 50, maxLossPct: 2.5 });
  });

  it("lists known latency stages in order", () => {
    const stats = { captureMs: 0, encodeMs: 10, networkMs: 2, bufferMs: 30, outputMs: 12 } as RouteStats;
    expect(latencyStages(stats).map((s) => s.id)).toEqual(["encode", "network", "buffer", "output"]);
  });
});
