# ADR-0010: Single-process desktop with the engine independent of the window

- Status: accepted
- Date: 2026-09-14
- Plan: §13.1, §33.4, decision log ADR-0010

## Context

SoundPush runs all day in the tray and must accept streams with no window open. A separate background daemon plus
a UI process is the classic design, but it needs its own IPC protocol, service installation per OS, versioning
between the two processes, and more failure modes.

## Decision

- One process, single instance. The engine starts first (on a background thread, so the window never waits for it),
  then the tray. The main window is created only when needed and **destroyed when closed**, freeing the webview's memory.
- The UI talks to the engine only through typed Tauri commands and a state event, the same boundary a separate
  daemon would use.
- The engine and tray keep working if the webview fails to start.
- On quit, the engine is shut down explicitly (unmute speakers, tell peers, release sleep prevention) because Tauri
  exits without dropping managed state.

## Consequences

- Idle tray mode costs little memory and CPU (§22.1 budgets).
- Launch at sign-in starts `--autostart` without a window.
- A crash in the UI layer can take down streaming, since they share a process; panics in pipelines are supervised
  (`panic = "unwind"`).
- Splitting into a daemon later (for a headless Linux receiver or a local API) needs no engine API changes.
