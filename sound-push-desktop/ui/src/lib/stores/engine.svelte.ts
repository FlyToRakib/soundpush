// Reactive engine state shared by all screens (Svelte 5 runes).
import { onStartError, onState } from "../engine/client";
import type { EngineState, PeerView, RouteKind } from "../engine/types";
import { applyTheme } from "../theme/theme";
import { history } from "./history.svelte";

class EngineStore {
  state = $state<EngineState | null>(null);
  /** Set when the engine failed to start; the app shows it instead of "Starting…". */
  startError = $state<string | null>(null);

  constructor() {
    void onState((s) => {
      this.state = s;
      applyTheme(s.settings.theme);
      history.record(s.routes);
    });
    void onStartError((message) => (this.startError = message));
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
