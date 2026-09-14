# ADR-0016: F-Droid-compatible Android build

- Status: accepted
- Date: 2026-09-14
- Plan: §14.4, §31, §32, decision log ADR-0016

## Context

F-Droid only publishes apps built entirely from free software, with no proprietary Google libraries. Google Play
Services, Play Core (in-app updates, review prompts) and ML Kit (barcode scanning) are proprietary.

## Decision

- QR scanning uses CameraX + **ZXing** core (Apache-2.0), not ML Kit.
- No Play Core in-app updates. The app runs its own lightweight version check against GitHub Releases (Settings →
  About), which only links to the download page and can be turned off. It also shows "Update needed" when a paired
  device speaks an incompatible protocol version.
- No Firebase, analytics or crash SDKs (ADR-0013).
- Native code (Rust engine via UniFFI) builds from source with the pinned NDK.
- Distribution: Google Play (AAB), F-Droid, and signed APKs on GitHub Releases.

## Consequences

- One codebase serves all stores; no flavour-specific proprietary code.
- F-Droid lists self-update checks as a possible anti-feature; an F-Droid build can default
  "Check for updates automatically" to off if the maintainers of the F-Droid listing ask for it.
- QR scanning is slightly larger than ML Kit's on-device model; ZXing handles SoundPush's high-contrast codes well.
