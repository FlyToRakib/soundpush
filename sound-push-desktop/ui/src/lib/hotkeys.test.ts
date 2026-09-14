import { describe, expect, it } from "vitest";
import { formatAccelerator, recordKey, type KeyInput } from "./hotkeys";

const key = (code: string, mods: Partial<KeyInput> = {}): KeyInput => ({
  key: "",
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("hotkey recording", () => {
  it("builds accelerators from modifiers and a key", () => {
    expect(recordKey(key("KeyM", { ctrlKey: true, shiftKey: true }))).toEqual({ kind: "set", accelerator: "Ctrl+Shift+M" });
    expect(recordKey(key("Digit5", { altKey: true }))).toEqual({ kind: "set", accelerator: "Alt+5" });
  });

  it("allows function keys alone but not plain letters", () => {
    expect(recordKey(key("F13"))).toEqual({ kind: "set", accelerator: "F13" });
    expect(recordKey(key("KeyA"))).toEqual({ kind: "pending" });
    expect(recordKey(key("ShiftLeft", { shiftKey: true }))).toEqual({ kind: "pending" });
  });

  it("cancels and clears", () => {
    expect(recordKey(key("Escape"))).toEqual({ kind: "cancel" });
    expect(recordKey(key("Backspace"))).toEqual({ kind: "clear" });
  });

  it("formats for each platform", () => {
    expect(formatAccelerator("Ctrl+Shift+M", false)).toBe("Ctrl + Shift + M");
    expect(formatAccelerator("Ctrl+Shift+M", true)).toBe("⌃⇧M");
    expect(formatAccelerator("Super+F1", false)).toBe("Win + F1");
  });
});
