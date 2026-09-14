# ADR-0009: Symmetric device/route model instead of server/player roles

- Status: accepted
- Date: 2026-09-14
- Plan: §9.2, §15.8, §19, decision log ADR-0009

## Context

AudioRelay splits every setup into a "server" and a "player", and users must learn which side is which before
anything plays. SoundPush has more directions: PC → phone, phone mic → PC, phone apps → PC, phone → phone,
PC ↔ PC, and headset mode (two directions at once). A role-based model multiplies modes and UI.

## Decision

- Every installation is a **device** with **sources** (system audio, app audio, microphone) and **sinks**
  (speaker, virtual microphone), advertised as capabilities.
- A **route** connects one source on one device to one sink on another, with a profile (codec, bitrate, latency).
  A **session** is the authenticated connection between two devices that carries any number of routes.
- Either side may start a route; the other side checks its permissions (Allow / Ask / Deny) before audio flows.
- Tasks in the UI ("Use phone as microphone", "Headset") are named combinations of route kinds.
- Route graph rules: no cycles, one active feed per virtual microphone, a source may feed many sinks.

## Consequences

- The UI never says "server" or "player"; tasks describe outcomes.
- New sources and sinks (file recorder, network radio, AirPlay bridge) plug in without new modes (§33.1).
- Multi-device streaming is the same model with more routes.
- Route kinds are named from the local device's point of view (`sendSystemAudio`, `receiveMicToVirtualMic`), which
  both apps must mirror when showing requests from the other side.
