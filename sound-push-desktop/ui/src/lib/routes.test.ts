import { describe, expect, it } from "vitest";
import type { EngineState } from "./engine/types";
import { TASKS, platformIcon, routeIcon } from "./routes";

function stateWith(caps: Partial<EngineState["capabilities"]>): EngineState {
  return {
    capabilities: { systemAudio: false, appAudio: false, microphone: true, speaker: true, virtualMic: false, ...caps },
  } as EngineState;
}

describe("home tasks", () => {
  it("shows at most four tasks", () => {
    expect(TASKS.length).toBeLessThanOrEqual(4);
  });

  it("requires system audio capture to send computer audio", () => {
    const send = TASKS.find((t) => t.id === "sendSystemAudio")!;
    expect(send.available(stateWith({}))).toBe(false);
    expect(send.available(stateWith({ systemAudio: true }))).toBe(true);
  });

  it("requires both capture and a virtual mic for headset mode", () => {
    const headset = TASKS.find((t) => t.id === "headset")!;
    expect(headset.kinds).toEqual(["sendSystemAudio", "receiveMicToVirtualMic"]);
    expect(headset.available(stateWith({ systemAudio: true }))).toBe(false);
    expect(headset.available(stateWith({ systemAudio: true, virtualMic: true }))).toBe(true);
  });
});

describe("icons", () => {
  it("maps route kinds and platforms", () => {
    expect(routeIcon("receiveMicToVirtualMic")).toBe("mic");
    expect(routeIcon("receiveAppAudio")).toBe("apps");
    expect(routeIcon("sendSystemAudio")).toBe("speaker");
    expect(platformIcon("android")).toBe("phone");
    expect(platformIcon("windows")).toBe("laptop");
  });
});
