import { describe, expect, it } from "vitest";
import { auditText } from "./audit";
import type { AuditEntry } from "./engine/types";

const t = (key: string, ...args: (string | number)[]) => [key, ...args].join("|");
const entry: AuditEntry = { timeUnix: 0, kind: "pairingSucceeded", peerName: "Phone", peerCode: "SP-1234", detail: "" };

describe("security log text", () => {
  it("names the device, route and permission", () => {
    expect(auditText(entry, t)).toBe("audit.pairingSucceeded|Phone");
    expect(auditText({ ...entry, kind: "routeStarted", route: "sendMicToVirtualMic", detail: "peer" }, t)).toBe(
      "audit.routeStarted|route.sendMicToVirtualMic|Phone",
    );
    expect(auditText({ ...entry, kind: "permissionChanged", detail: "useMyMicrophone=deny" }, t)).toBe(
      "audit.permissionChanged|Phone|perm.useMyMicrophone|policy.deny",
    );
  });

  it("falls back to the device code and known reasons", () => {
    expect(auditText({ ...entry, kind: "deviceBlocked", peerName: "" }, t)).toBe("audit.deviceBlocked|SP-1234");
    expect(auditText({ ...entry, kind: "pairingRejected", detail: "proof" }, t)).toBe("audit.pairingRejected.proof|Phone");
    expect(auditText({ ...entry, kind: "pairingRejected", detail: "???" }, t)).toBe("audit.pairingRejected.closed|Phone");
    expect(auditText({ ...entry, kind: "pairingRateLimited", detail: "192.168.1.9" }, t)).toBe(
      "audit.pairingRateLimited|192.168.1.9",
    );
    expect(auditText({ ...entry, kind: "logCleared", peerName: "", peerCode: "" }, t)).toBe(
      "audit.logCleared|audit.unknownDevice",
    );
  });
});
