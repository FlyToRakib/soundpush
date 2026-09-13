// Reactive engine state shared by all screens (Svelte 5 runes).
import { onState } from "../engine/client";
import type { EngineState, PeerView, RouteKind } from "../engine/types";
import { applyTheme } from "../theme/theme";

class EngineStore {
  state = $state<EngineState | null>(null);

  constructor() {
    void onState((s) => {
      this.state = s;
      applyTheme(s.settings.theme);
    });
  }

  get trustedPeers(): PeerView[] {
    return this.state?.peers.filter((p) => p.trusted) ?? [];
  }

  get connectedPeers(): PeerView[] {
    return this.trustedPeers.filter((p) => p.connection === "connected");
  }

  get nearbyUntrusted(): PeerView[] {
    return this.state?.peers.filter((p) => !p.trusted && p.online) ?? [];
  }

  /** Connected peers able to take part in a route of this kind (from this device's perspective). */
  peersFor(kind: RouteKind): PeerView[] {
    return this.connectedPeers.filter((p) => {
      switch (kind) {
        case "sendSystemAudio":
        case "sendAppAudio":
        case "sendMicToSpeaker":
          return p.canPlay;
        case "sendMicToVirtualMic":
          return p.hasVirtualMic;
        case "receiveSystemAudio":
          return p.canSendSystemAudio;
        case "receiveAppAudio":
          return p.canSendAppAudio;
        case "receiveMicToVirtualMic":
        case "receiveMicToSpeaker":
          return p.canSendMic;
      }
    });
  }
}

export const store = new EngineStore();
