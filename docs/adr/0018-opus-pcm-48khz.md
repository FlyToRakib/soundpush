# ADR-0018: Opus and PCM codecs with a fixed 48 kHz wire format

- Status: accepted
- Date: 2026-09-14
- Plan: §10.6, §15.1, decision log ADR-0005 (renumbered, see README)

## Context

SoundPush streams music, system sound and speech over Wi-Fi and USB with low latency. Devices run at 44.1 or
48 kHz with different channel layouts. Negotiating arbitrary sample rates per route complicates every stage
(jitter buffer, drift compensation, redundancy).

Alternatives considered: AAC (patents, higher delay), FLAC (lossless but larger and higher delay than PCM on a LAN),
variable wire rates (more code paths, more bugs).

## Decision

- The wire format is always **48 kHz**, 1 or 2 channels, 10 ms frames by default (5 ms Low latency, 20 ms Stable).
  Capture and render edges resample; more than two channels are downmixed at the source.
- **Opus** is the default codec: low delay (restricted low-delay mode for music, VoIP mode for microphone routes),
  6–510 kb/s, royalty-free, built-in packet-loss concealment. Adaptive bitrate follows network conditions.
- **PCM** (16-bit / float) is available as "Lossless" for USB and strong LANs.
- The codec sits behind a `Codec` trait so FLAC or LC3 can be added later.
- The Opus implementation is the pure-Rust translation of libopus (`unsafe-libopus`) so no C toolchain is needed.

## Consequences

- One pipeline shape for every route; drift compensation and jitter buffering assume 48 kHz.
- Resampling costs a little CPU on 44.1 kHz devices.
- PCM at 48 kHz stereo 16-bit is ~1.5 Mb/s, fine for USB and good Wi-Fi but not for weak links; "Automatic"
  quality prefers Opus.
