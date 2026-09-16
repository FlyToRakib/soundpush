import type { IconName } from "./components/Icon.svelte";
import type { EngineState, RouteKind, RouteView } from "./engine/types";
import { t } from "./i18n";

export interface Task {
  id: string;
  icon: IconName;
  kinds: RouteKind[];
  available: (s: EngineState) => boolean;
  unavailableKey?: string;
}

/** Home-screen tasks, from this computer's perspective. A handful at most. */
export const TASKS: Task[] = [
  {
    id: "sendSystemAudio",
    icon: "speaker",
    kinds: ["sendSystemAudio"],
    available: (s) => s.capabilities.systemAudio,
    unavailableKey: "task.unavailable.loopback",
  },
  {
    id: "receiveMicToVirtualMic",
    icon: "mic",
    kinds: ["receiveMicToVirtualMic"],
    available: (s) => s.capabilities.virtualMic,
    unavailableKey: "task.unavailable.virtualMic",
  },
  {
    id: "headset",
    icon: "headset",
    kinds: ["sendSystemAudio", "receiveMicToVirtualMic"],
    available: (s) => s.capabilities.systemAudio && s.capabilities.virtualMic,
    unavailableKey: "task.unavailable.virtualMic",
  },
  {
    id: "receiveAppAudio",
    icon: "apps",
    kinds: ["receiveAppAudio"],
    available: () => true,
  },
  // Commentary over a game, for example: one stream carrying both (plan §5.1).
  {
    id: "sendMixed",
    icon: "mic",
    kinds: ["sendMixed"],
    available: (s) => s.capabilities.mixed,
    unavailableKey: "task.unavailable.loopback",
  },
];

export function routeTitle(route: RouteView): string {
  return t(`route.${route.kind}`, route.peerName);
}

export function routeIcon(kind: RouteKind): IconName {
  if (kind.includes("Mic")) return "mic";
  if (kind.includes("App")) return "apps";
  return "speaker";
}

export function platformIcon(platform: string): IconName {
  return platform === "android" || platform === "ios" ? "phone" : "laptop";
}
