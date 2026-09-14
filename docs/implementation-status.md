# Implementation status

Tracks [`soundpush-final.md`](soundpush-final.md) against the code. Updated 2026-09-13.

Legend: ✅ done and tested · 🟡 implemented, needs real-device verification · ⏳ not started · ⛔ blocked on something outside the repo

## Phase 0 — Foundations

| Item | Status | Notes |
|---|---|---|
| Monorepo, Cargo/npm/Gradle workspaces, `justfile`, CI workflow | ✅ | `.github/workflows/ci.yml` (not yet run on GitHub) |
| Design tokens → CSS + Compose, WCAG contrast check | ✅ | `design/scripts/generate.mjs` |
| Wire protocol spec, pairing/security doc, ADRs 0001–0005 | ✅ | `docs/protocol`, `docs/security`, `docs/adr` |
| Open-source files (LICENSE notice, CONTRIBUTING, CoC, SECURITY) | ✅ | Full GPL text must be pasted into `LICENSE` before release |
| Fuzz targets (media, control, framing, QR, beacon) | ✅ | `fuzz/` — run with `cargo +nightly fuzz run <target>` |

## Phase 1 — Core engine + "Listen to PC on phone"

| Item | Status | Notes |
|---|---|---|
| `sp-protocol` (messages, media header, framing, versions) | ✅ | 12 tests |
| `sp-security` (identity, QR proof, SAS, encrypted trust store) | ✅ | 13 tests |
| `sp-transport` (QUIC, mutual TLS, pinning, datagrams, exporter) | ✅ | real QUIC tests |
| `sp-discovery` (mDNS, signed beacons, registry, ranking) | 🟡 | unit-tested; multicast needs real-network check |
| `sp-media` (Opus/PCM, adaptive jitter buffer, drift, DSP, RNNoise) | ✅ | 19 tests incl. 1-hour drift simulation |
| `sp-audio-io` (trait, cpal backend, null backend, converters) | 🟡 | unit-tested; device paths need hardware |
| `sp-engine` (actor, sessions, pairing, routes, permissions, reconnect, settings, state) | ✅ | 13 tests incl. two-engine pairing → audio → prompt → stop → forget → restart-reconnect |
| Desktop app (Tauri shell, tray, autostart, sleep inhibit, diagnostics, UI) | 🟡 | builds and launches on macOS (identity in Keychain, listening on UDP 47650); svelte-check clean, 9 UI tests |
| Android app (engine FFI, service, notifications, tile, QR scan, screens) | 🟡 | debug APK builds (arm64); needs a real phone to test |

## Phase 2 — Microphone & headset

| Item | Status | Notes |
|---|---|---|
| Phone mic → PC virtual mic (compatibility mode: VB-CABLE/BlackHole/Voicemeeter) | 🟡 | ADR-0003 |
| Android mic presets (7 modes) + platform AEC/NS/AGC with availability | 🟡 | Kotlin `MicCapture` |
| Gain 0–20 dB, soft limiter, RNNoise, level meter, mic monitor | ✅ | |
| "Ask" permission prompt for microphone | ✅ | tested end-to-end |
| Headset mode (both routes in one action) | 🟡 | desktop + Android home task |
| Windows stage 1: VB-CABLE bundled in the SoundPush installer (silent install, credit line, restart prompt) | ⛔ | waiting for VB-Audio's written agreement (required by the licence in the package); pinned download script ready. See [virtual-microphone.md](virtual-microphone.md) §4 |
| Windows stage 2: own "SoundPush Microphone" driver in the repo, built + test-signed in CI | ⏳ | replaces VB-CABLE once Microsoft-signed |
| Windows stage 2: Microsoft attestation signing of the driver | ⛔ | needs an EV code-signing cert + Partner Center account (AudioRelay's driver is signed this way) |
| Linux PipeWire virtual source created by the app | ⏳ | |
| Push-to-talk / mute global hotkey | 🟡 | `src-tauri/src/hotkeys.rs` (global-shortcut plugin), recorder on the Audio page, conflict errors, tray check mark follows the engine's `micMuted`; mute also silences the phone mic feeding the virtual mic |
| Auto-start phone mic when an app opens the virtual mic | 🟡 | `desktop.autoStartMic`; `PlatformHooks::virtual_mic_in_use` (Windows: active sessions on "CABLE Output"; macOS: `DeviceIsRunningSomewhere`); engine `actor/local_audio.rs` starts the last mic phone and stops it 15 s after the app lets go. Linux: call site in `hooks.rs` |

## Phase 3 — Parity completion

| Item | Status | Notes |
|---|---|---|
| Android app audio → PC (playback capture) | 🟡 | `AppAudioCapture` |
| PC mic → phone, PC↔PC, phone↔phone | 🟡 | same route model; untested on hardware |
| Multi-device streaming | 🟡 | engine supports N sessions/routes; encoder sharing ⏳ |
| Per-stream volume, balance, mono, A/V offset, remote volume/mute, "Mute PC" | ✅ / 🟡 | engine + UI; OS mute via platform calls |
| Adaptive bitrate from receiver stats | ✅ | |
| Packet redundancy (auto on >1 % loss) | ✅ | test recovers frames with 20 % simulated loss |
| Session timer, quality badge, connection details | ✅ | |
| Custom bitrate steps incl. AudioRelay's | ✅ | |
| USB tethering | 🟡 | works as IP network |
| USB via ADB (TLS-over-TCP transport) | ⏳ | ADR-0005 |
| Windows per-app capture | 🟡 | `sp-audio-io/src/wasapi_process.rs` (process loopback, one app or everything except one app, Windows 10 2004+); app picker on the Audio page |
| PipeWire app capture | ⏳ | ADR-0002 |
| Audio focus (pause/duck/mix), headphone-unplug pause, auto-resume | 🟡 | Android service |
| Quick Settings tile, reboot reminder, OEM battery guide link | 🟡 | |
| Android "output audio effects" / compatibility output | ⏳ | needs non-cpal playback path |
| Media Feature Pack detection (Windows N) | 🟡 | `src-tauri/src/system.rs` (missing `mfplat.dll`), Home banner → Optional features |
| Windows Firewall / Public network fix | 🟡 | `src-tauri/src/network.rs`: firewall policy + network category read as a normal user; "Allow SoundPush" runs `netsh` through UAC (UDP, this exe, private/domain; public only if chosen); the uninstaller removes the rule with one UAC prompt (`windows/hooks.nsh`) |
| Audio device changes (follow default, device lost) | 🟡 | `src-tauri/src/device_watch.rs` (`IMMNotificationClient`, Core Audio listeners) → `EngineHandle::audio_devices_changed`; routes on the default device reopen, pinned devices report lost |
| Audio cues | 🟡 | `src-tauri/src/cues.rs`, generated tones, `settings.audioCues` |
| Diagnostics export, guided troubleshooter, logs | ✅ (desktop) / 🟡 (Android tips only) | desktop troubleshooter runs checks (firewall, network profile, driver, devices, permissions, Bluetooth) with fix buttons |
| macOS permissions (microphone, System Audio Recording) | 🟡 | `src-tauri/src/macos.rs`: status without prompting, deep links to System Settings. Not yet compiled on macOS |
| Windows ARM64 installer | 🟡 | `windows-build.yml` matrix, artifact `SoundPush-Windows-arm64` (cross-compiled, not yet run) |
| i18n infrastructure (English) | ✅ | translations via community later |
| Accessibility pass with screen readers | ⏳ | components are labelled; audit pending |

## Phase 4 — macOS parity

| Item | Status | Notes |
|---|---|---|
| Desktop app runs on macOS (speaker, mic, pairing) | 🟡 | runs on this Mac; phone paired and played audio through the Mac speakers |
| System-audio capture via process taps | 🟡 | `sp-audio-io/src/macos_tap.rs`: tap + aggregate device open and deliver frames; real audio needs the System Audio Recording permission, which macOS grants only to the bundled `SoundPush.app` (unbundled dev binaries receive silence). Minimum macOS raised to 14.2. |
| AudioServerPlugIn "SoundPush Microphone" | 🟡 | own C driver in `sound-push-desktop/drivers/macos-virtual-mic`, embedded in the app and installed from the Audio page; needs a real install test. Public distribution needs Apple Developer ID notarization |

## Phase 5 — Hardening & release

| Item | Status |
|---|---|
| Real-device matrix, 24 h soak, battery measurement | ⏳ (needs devices) |
| External security review | ⏳ |
| Installers (NSIS/deb/rpm/AppImage/DMG), updater signing, store listings | ⛔ (signing keys/accounts) |
