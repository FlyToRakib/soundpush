# ADR-0008: PipeWire native virtual source on Linux

- Status: accepted (implementation in progress)
- Date: 2026-09-14
- Plan: §13.3, decision log ADR-0008

## Context

On Linux, apps like Discord, Zoom and OBS pick microphones from PipeWire (directly or through pipewire-pulse).
Unlike Windows and macOS, a virtual input device needs no kernel driver or signing: a user-space process can
publish an `Audio/Source` node. The alternative, telling users to run `pactl load-module module-null-sink …`,
is error-prone, does not survive reboots, and contradicts "users never install or configure extra software".

## Decision

- The engine creates a PipeWire node named **SoundPush Microphone** (`media.class = Audio/Source`) when the
  microphone route needs it, and removes it when the engine exits. The phone's microphone audio is written into it.
- PipeWire ≥ 0.3.48 is the supported target (Ubuntu LTS, Fedora).
- PulseAudio-only systems: load `module-null-sink` + `module-remap-source` through libpulse on demand and unload
  them on exit, so the user still sees "SoundPush Microphone" without manual commands.
- Detection follows ADR-0003: the engine uses the SoundPush source when present and never falls back to a speaker.

## Consequences

- Zero manual setup on mainstream distributions; no root access.
- The desktop crate gains optional `pipewire`/`libpulse-binding` dependencies on Linux; CI and release builds
  install their development packages.
- Flatpak builds need the PipeWire socket permission.
- Until this lands, the Audio page on Linux explains that the virtual microphone is not available yet.
