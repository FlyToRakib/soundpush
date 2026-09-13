# ADR-0002: Audio backends — cpal first, platform capture where cpal falls short

- Status: accepted
- Date: 2026-09-13

## Context

The plan names WASAPI, PipeWire, CoreAudio and Oboe backends. Writing four native backends before
anything plays would delay feedback, and cpal already wraps all of them (0.16 uses AAudio directly on
Android with no C++ build).

## Decision

- `sp-audio-io` defines the `AudioBackend` trait (48 kHz float at the boundary) with a cpal implementation.
- Windows system audio uses WASAPI loopback through cpal.
- Android microphone and app-audio capture use Kotlin `AudioRecord` (input presets, platform AEC/NS/AGC,
  playback capture) and push PCM into the engine; playback uses cpal/AAudio.
- Native backends replace cpal behind the same trait where it cannot deliver a planned feature:
  Windows per-app process loopback, PipeWire virtual source, macOS process taps.

## Consequences

- Speaker and microphone streaming work on all platforms now.
- macOS system audio uses a Core Audio process tap (excluding SoundPush itself) wrapped in a temporary
  public aggregate device that cpal opens as an input (`sp-audio-io/src/macos_tap.rs`). The tap must be
  public too, or it will not attach to the aggregate. Requires macOS 14.2+ and the System Audio Recording
  permission (`NSAudioCaptureUsageDescription`); without it macOS delivers silence rather than an error.
- System-audio capture on Linux is unavailable until the PipeWire backend lands (the UI says so).
- The Opus codec uses `unsafe-libopus` (pure-Rust libopus translation) so no CMake/C toolchain is required.
