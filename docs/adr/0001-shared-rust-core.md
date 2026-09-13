# ADR-0001: Shared Rust core

- Status: accepted
- Date: 2026-09-13

## Context

Desktop and mobile must speak the same protocol, pairing and jitter-buffer logic exactly.
Network-facing parsers handle untrusted LAN input. Audio paths must avoid GC pauses.

## Decision

Protocol, security, transport, discovery, media DSP and the session/route engine live in a Rust
workspace (`sound-push-core`). Desktop links it directly (Tauri); Android uses UniFFI bindings
with the full state crossing the boundary as JSON to keep the FFI surface small.

## Consequences

- One implementation, tested once (two-engine integration tests cover pairing → audio → reconnect).
- Mobile builds need the NDK and `cargo-ndk`.
- Kotlin decodes state JSON; unknown fields are ignored so the engine can evolve independently.
