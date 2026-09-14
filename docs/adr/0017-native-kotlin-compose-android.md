# ADR-0017: Native Kotlin + Jetpack Compose Android app

- Status: accepted
- Date: 2026-09-14
- Plan: §10.4, §14, decision log ADR-0003 (renumbered, see README)

## Context

The Android app depends on deep OS integration: foreground service types that change with active routes,
MediaProjection playback capture, audio focus, input presets and audio effects, Quick Settings tiles, OEM battery
quirks, and TalkBack. The audio hot path must not cross a slow bridge per buffer.

Alternatives considered: Flutter and React Native (every OS feature becomes a platform-channel plugin; two languages
anyway; a bridge in exactly the latency-sensitive part), Kotlin Multiplatform UI shared with desktop (the engine is
Rust, so KMP would duplicate it).

## Decision

- Kotlin + Jetpack Compose (Material 3) with a single activity and Navigation Compose; unidirectional data flow from
  the engine state to the UI.
- The shared Rust engine is linked through UniFFI (`sp-ffi`), built with `cargo-ndk`; state crosses as JSON.
- Microphone and app-audio capture use `AudioRecord` in Kotlin and push PCM into the engine; playback runs in native
  code (ADR-0002).
- Modules: `app`, `core-engine`, `core-ui`, `feature-*`, `platform-service`; the theme is generated from the shared
  design tokens.

## Consequences

- Full access to Android APIs with first-class accessibility (TalkBack) and no extra runtime.
- The UI is written twice (Svelte on desktop, Compose on Android); only the engine and design tokens are shared.
- Builds need the Android NDK and a Rust toolchain; CI generates bindings and native libraries before Gradle runs.
