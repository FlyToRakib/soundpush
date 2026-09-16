# Architecture Decision Records

An ADR records one significant, cross-cutting decision: the context, what was decided, and the consequences.
Write one before a large change (CONTRIBUTING.md). Copy the layout of an existing ADR; never edit an accepted
decision silently. Supersede it with a new ADR or mark it "revised" with the reason.

The plan's decision log (docs/soundpush-final.md §37) numbered its seed ADRs before the repository existed. Some
repository numbers were used for spike results first, so the table maps both.

| File | Decision | Plan §37 ID | Status |
|---|---|---|---|
| [0001](0001-shared-rust-core.md) | Shared Rust core | ADR-0001 | Accepted |
| [0002](0002-audio-backends.md) | Audio backends: cpal first, platform capture where needed | (spike result) | Accepted |
| [0003](0003-virtual-microphone.md) | Virtual microphone: built-in on macOS, VB-CABLE then own driver on Windows | revises ADR-0007 | Accepted (revised) |
| [0004](0004-desktop-tauri-svelte.md) | Desktop shell: Tauri 2 + Svelte 5 | ADR-0002 | Accepted |
| [0005](0005-quic-transport.md) | QUIC transport with key-pinned mutual TLS | ADR-0004 | Accepted |
| [0006](0006-key-pinned-pairing.md) | Key-pinned pairing via QR secret or TLS-exporter SAS | ADR-0006 | Accepted |
| [0007](0007-own-windows-virtual-audio-driver.md) | SoundPush's own Windows virtual audio driver | ADR-0007 | Accepted, deferred |
| [0008](0008-pipewire-virtual-source.md) | PipeWire native virtual source on Linux | ADR-0008 | Accepted |
| [0009](0009-symmetric-route-model.md) | Symmetric device/route model | ADR-0009 | Accepted |
| [0010](0010-single-process-desktop.md) | Single-process desktop, engine independent of the window | ADR-0010 | Accepted |
| [0011](0011-manual-di-android.md) | Manual dependency injection on Android | ADR-0011 | Accepted |
| [0012](0012-settings-and-trust-in-engine.md) | Settings and trust stored in the engine | ADR-0012 | Accepted |
| [0013](0013-fully-free-no-telemetry.md) | Fully free: no ads, paid tiers or telemetry | ADR-0013 | Accepted |
| [0014](0014-gpl-3-license.md) | GPL-3.0-or-later license | ADR-0014 | Accepted |
| [0015](0015-macos-in-1-0.md) | macOS included in 1.0 | ADR-0015 | Accepted |
| [0016](0016-f-droid-compatible-android.md) | F-Droid-compatible Android build | ADR-0016 | Accepted |
| [0017](0017-native-kotlin-compose-android.md) | Native Kotlin + Compose Android app | ADR-0003 | Accepted |
| [0018](0018-opus-pcm-48khz.md) | Opus and PCM codecs, 48 kHz wire format | ADR-0005 | Accepted |
| [0019](0019-release-signing-without-paid-certificates.md) | Releases and updates without paid certificates | (new) | Accepted |
| [0020](0020-desktop-echo-control.md) | Half-duplex echo control on the desktop instead of a bundled AEC | (new) | Accepted |
