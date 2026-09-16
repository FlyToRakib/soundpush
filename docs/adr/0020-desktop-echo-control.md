# ADR-0020: Half-duplex echo control on the desktop instead of a bundled AEC

- Status: accepted
- Date: 2026-09-16
- Plan: §5.1 (Headset mode), §15.7 (microphone processing), §8.2 (feedback loops)

## Context

Headset mode is full duplex: the computer sends its system audio to the phone and plays the phone's microphone
back on the computer. Android has an acoustic echo canceller in the platform (`AcousticEchoCanceler`, selected
through the `VoiceCommunication` microphone preset), so the phone side is covered. The desktop has nothing
equivalent. When the desktop plays the far side through **speakers** rather than headphones, its own microphone
picks that audio up and sends it back, so the person on the phone hears themselves a fraction of a second late.

The plan (§15.7) left the choice open: "Fallback: WebRTC AEC3 in `sp-media` (Phase 3 spike decides whether
platform AEC is sufficient across the device matrix)."

What a real acoustic echo canceller needs is a far-end reference signal, an adaptive filter (typically a
partitioned-block frequency-domain filter), double-talk detection and residual echo suppression. Options
evaluated for the desktop:

| Option | Verdict |
|---|---|
| `webrtc-audio-processing` (the crate, bindings to WebRTC APM) | Needs a C++ toolchain plus `meson`/`ninja` or a vendored abseil at build time. On Windows that means MSVC plus Python-based build tools in CI, and MinGW builds are unsupported. It would break at least one of our three platforms and add a large vendored C++ tree. |
| `speexdsp` / `speexdsp-sys` (`speex_echo_state`) | Needs a C compiler and either a system library or a vendored copy; the same CI problem, and its canceller is weak against the delays a USB or Bluetooth output adds. |
| A pure-Rust AEC | None exists that is mature enough to ship. Writing an adaptive-filter AEC with double-talk handling is a project in itself, and getting it wrong is worse than not having one (it damages speech). |
| Platform AEC on the desktop | Windows offers an APO-based voice-capture mode that few devices implement usefully and which forces mono 16 kHz; macOS exposes `kAUVoiceIOProperty_*` only through Audio Units, not through `cpal`. Neither is available on Linux. |

The brief's own constraint — "do not add a dependency that breaks CI builds on any platform" — rules out every
option that needs a new native toolchain.

## Decision

No echo canceller is bundled on the desktop in 0.1. Instead:

1. **Detect the problem.** The microphone monitor runs a feedback detector (`sp_media::dsp::FeedbackDetector`):
   a loud, near-sinusoidal tone that holds its pitch while its level builds is reported as a howl. Speech and
   room noise are not, so the warning stays trustworthy. The engine then tells the user once and suggests
   headphones or Headset mode (plan §8.2).
2. **Offer a mitigation that cannot damage speech.** `mic.echoDucking` ("Reduce echo on speakers") is
   half-duplex control: while this device plays the far side on its own speakers, the microphone is attenuated
   by 18 dB with a hold over word gaps (`sp_media::dsp::DuckGate`). Every receiver rendering to a speaker
   publishes its level into a shared `EchoReference`, which the microphone sender reads once per frame, so the
   duck reacts within about 10 ms. It is **off by default**, because it makes the conversation half duplex and
   the right answer is usually headphones.
3. **Keep pointing at headphones.** The monitor description, the feedback warning and Headset mode all say so.
   On Android nothing changes: the platform AEC keeps doing the job.

## Consequences

- No new native toolchain, no new C/C++ dependency, and CI keeps building on Windows, macOS and Linux
  unchanged.
- Desktop Headset mode on speakers is usable with "Reduce echo on speakers" on, but it is half duplex: the
  microphone is quiet while the other side talks. Headphones remain the recommended setup and stay full duplex.
- The duck is a plain gain, so it can never distort or cancel wanted speech the way a misconverged adaptive
  filter can; the worst case is that the user's own voice is briefly quieter.
- If a maintained pure-Rust or vendored-without-toolchain AEC appears, it can replace the duck behind the same
  setting: the far-end reference (`EchoReference`) and the place it is applied in the sender pipeline are
  already there. That would supersede this ADR.
