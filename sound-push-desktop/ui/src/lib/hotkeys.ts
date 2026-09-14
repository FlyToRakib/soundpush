// Global shortcut recording: keyboard events → accelerator strings understood by
// src-tauri/src/hotkeys.rs (e.g. "Ctrl+Shift+M", "F13").

export interface KeyInput {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;

/** Key part of an accelerator from `KeyboardEvent.code` (layout independent), or null. */
function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (/^Numpad[0-9]$/.test(code)) return code;
  const named: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    ArrowUp: "ArrowUp",
    ArrowDown: "ArrowDown",
    ArrowLeft: "ArrowLeft",
    ArrowRight: "ArrowRight",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    Insert: "Insert",
    Pause: "Pause",
    ScrollLock: "ScrollLock",
    Minus: "Minus",
    Equal: "Equal",
    BracketLeft: "BracketLeft",
    BracketRight: "BracketRight",
    Backslash: "Backslash",
    Semicolon: "Semicolon",
    Quote: "Quote",
    Backquote: "Backquote",
    Comma: "Comma",
    Period: "Period",
    Slash: "Slash",
  };
  return named[code] ?? null;
}

export type RecordResult = { kind: "set"; accelerator: string } | { kind: "clear" } | { kind: "cancel" } | { kind: "pending" };

/**
 * Interpret a key press while recording. Escape cancels, Backspace/Delete without modifiers
 * clears. Letters, digits and most keys need a modifier (so typing is never captured
 * system-wide); F13–F24 and other F-keys work alone, as dedicated push-to-talk keys often are.
 */
export function recordKey(e: KeyInput): RecordResult {
  const modifiers = e.ctrlKey || e.altKey || e.shiftKey || e.metaKey;
  if (e.code === "Escape" && !modifiers) return { kind: "cancel" };
  if ((e.code === "Backspace" || e.code === "Delete") && !modifiers) return { kind: "clear" };
  if (MODIFIER_CODES.test(e.code)) return { kind: "pending" };
  const key = keyName(e.code);
  if (!key) return { kind: "pending" };
  const functionKey = /^F\d+$/.test(key);
  if (!modifiers && !functionKey) return { kind: "pending" };
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Super");
  parts.push(key);
  return { kind: "set", accelerator: parts.join("+") };
}

/** Human-readable accelerator ("Ctrl+Shift+M" → "Ctrl + Shift + M", or ⌃⇧M on a Mac). */
export function formatAccelerator(accelerator: string, mac: boolean): string {
  const parts = accelerator.split("+");
  if (mac) {
    const symbols: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘" };
    return parts.map((p) => symbols[p] ?? p).join("");
  }
  return parts.map((p) => (p === "Super" ? "Win" : p)).join(" + ");
}
