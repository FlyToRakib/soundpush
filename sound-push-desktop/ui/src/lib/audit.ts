import type { AuditEntry } from "./engine/types";

type Translate = (key: string, ...args: (string | number)[]) => string;

const REJECTIONS = ["proof", "closed", "local", "peer"];

/** One line of the security log. Each kind's `detail` is documented in sp-engine audit.rs. */
export function auditText(entry: AuditEntry, t: Translate): string {
  const device = entry.peerName || entry.peerCode || t("audit.unknownDevice");
  switch (entry.kind) {
    case "pairingAttempt":
      return t("audit.pairingAttempt", device, entry.detail);
    case "pairingRejected":
      return t(`audit.pairingRejected.${REJECTIONS.includes(entry.detail) ? entry.detail : "closed"}`, device);
    case "pairingRateLimited":
      return t("audit.pairingRateLimited", entry.detail);
    case "permissionChanged": {
      const [permission = "", policy = ""] = entry.detail.split("=");
      return t("audit.permissionChanged", device, t(`perm.${permission}`), t(`policy.${policy}`));
    }
    case "routeApproved":
    case "routeDenied":
    case "routeStarted":
    case "routeStopped":
      return t(`audit.${entry.kind}`, entry.route ? t(`route.${entry.route}`, device) : device);
    default:
      return t(`audit.${entry.kind}`, device);
  }
}
