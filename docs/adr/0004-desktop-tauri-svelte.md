# ADR-0004: Desktop shell — Tauri 2 + Svelte 5

- Status: accepted
- Date: 2026-09-13

## Decision

The desktop app is a single Tauri 2 process that links the Rust engine directly. The UI is Svelte 5 +
TypeScript rendered in the system webview. The engine starts before any window; the window is created on
demand and destroyed when closed, so tray mode costs almost nothing.

## Why

- Small installers and memory footprint versus Electron; no bundled Chromium.
- Web accessibility (screen readers, keyboard, RTL) is mature.
- Tray, autostart, single-instance and opener plugins are official.
- If the webview fails to start, streaming and the tray keep working.

## Consequences

- UI talks to Rust only through typed commands (`src-tauri/src/commands.rs`) and one state event.
- TypeScript state types mirror the engine's serde output (`ui/src/lib/engine/types.ts`).
- Theme (Light/Dark/System) is applied to both the webview and the native title bar.
