# SoundPush

Free and open-source audio routing between your computer and phone.

- Hear your **PC audio on your phone**
- Use your **phone as a PC microphone**
- Use your **phone as a wireless headset**
- Send **phone app audio** to a PC or another phone
- Stream to **multiple devices**, over Wi-Fi or USB

Secure pairing, end-to-end encrypted, no accounts, no ads, no telemetry.

## Repository layout

| Folder | What it is |
|---|---|
| `sound-push-core/` | Shared Rust engine: protocol, security, transport, discovery, audio DSP, sessions |
| `sound-push-desktop/` | Desktop app (Tauri 2 + Svelte) for Windows, Linux and macOS |
| `sound-push-mobile/` | Android app (Kotlin + Jetpack Compose) |
| `design/` | Design tokens shared by both apps (Light / Dark) |
| `docs/` | Plan, ADRs, protocol and security specifications |
| `tools/` | Test and measurement tools |

The full implementation plan is in [`docs/soundpush-final.md`](docs/soundpush-final.md).

## Building

```bash
just core-test      # build and test the shared Rust core
just desktop-dev    # run the desktop app in development mode
just mobile-build   # build the Android app (debug)
```

Requirements: Rust (see `rust-toolchain.toml`), Node 22 + pnpm, JDK 17, Android SDK + NDK, `cargo-ndk`.

## License

GPL-3.0-or-later. See `LICENSE`.
