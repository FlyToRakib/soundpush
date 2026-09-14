# ADR-0015: macOS included in 1.0

- Status: accepted
- Date: 2026-09-14
- Plan: §4.5, §13.3, Phase 4, decision log ADR-0015

## Context

AudioRelay ships a macOS app, so leaving macOS until after 1.0 would break the parity goal. macOS has the hardest
constraints: system-audio capture needs Core Audio process taps (macOS 14.2+) or ScreenCaptureKit, a virtual
microphone needs an AudioServerPlugIn, and public distribution needs Developer ID signing and notarisation.

## Decision

- macOS is a 1.0 platform with the same parity matrix as Windows (Phase 4).
- System audio uses a Core Audio process tap excluding SoundPush itself, wrapped in a temporary aggregate device
  (ADR-0002). Minimum macOS for the current build: 14.2.
- The virtual microphone is SoundPush's own AudioServerPlugIn, embedded in the app and installed from the Audio page
  with an administrator prompt (ADR-0003).
- Builds are universal (Apple Silicon + Intel). Until the project has a Developer ID, builds are ad-hoc signed with
  the audio-input entitlement, and users choose "Open Anyway" once (ADR-0019).

## Consequences

- CI builds macOS on every push (`macos-build.yml`) and in the release workflow.
- macOS permission flows (Microphone, Screen & System Audio Recording) need testing on each macOS release.
- Notarisation is required before recommending macOS builds widely (docs/release-signing.md §3).
- macOS 13 support via ScreenCaptureKit remains a planned fallback.
