# ADR-0003: Virtual microphone — compatibility mode first, signed driver next

- Status: accepted
- Date: 2026-09-13

## Context

"Phone as PC microphone" needs an OS input device that other apps can select. The plan's own
Windows driver (PortCls/WaveRT) requires the Windows Driver Kit and an EV code-signing certificate
for attestation signing; macOS needs a notarized AudioServerPlugIn. Neither can be produced without
the project's signing accounts.

## Decision

1. **Now:** the engine feeds any playback device that loops into a recording device. The desktop app
   auto-detects common ones (VB-CABLE "CABLE Input", BlackHole, Voicemeeter, and the future
   "SoundPush Microphone") or lets the user pick one under Audio → Virtual microphone.
2. **Next:** ship the SoundPush Windows driver and macOS HAL plug-in; both appear as
   "SoundPush Microphone" and are detected by the same code path. Linux creates a PipeWire source.

## Consequences

- The feature works today with a free third-party virtual cable.
- Releasing the native driver is blocked on obtaining signing certificates (tracked as a release blocker).
