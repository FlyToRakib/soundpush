# Implementation status

Tracks [`soundpush-final.md`](soundpush-final.md) against the code. Updated 2026-09-14.

Legend: ✅ done and tested · 🟡 implemented, needs real-device verification · ⏳ not started · ⛔ blocked on something outside the repo

## Phase 0 — Foundations

| Item | Status | Notes |
|---|---|---|
| Monorepo, Cargo/npm/Gradle workspaces, `justfile`, CI workflow | ✅ | `.github/workflows/ci.yml` (not yet run on GitHub) |
| Design tokens → CSS + Compose, WCAG contrast check | ✅ | `design/scripts/generate.mjs` |
| Wire protocol spec, pairing/security doc, threat model, ADRs 0001–0019 | ✅ | `docs/protocol`, `docs/security`, `docs/adr` (index maps the plan's ADR numbers) |
| Open-source files (full GPL-3.0 LICENSE, CONTRIBUTING, CoC, SECURITY, PRIVACY, CHANGELOG, issue/PR templates, CODEOWNERS, Dependabot) | ✅ | |
| CI quality gates: fmt, clippy, tests, svelte-check, vitest, `cargo deny`, `npm audit --audit-level=high`, Android unit tests + lint | 🟡 | `ci.yml`; `cargo fmt --check` fails on existing formatting (not yet reformatted) |
| Fuzz targets (media, control, framing, QR, beacon) | ✅ | `fuzz/` — run with `cargo +nightly fuzz run <target>` |

## Phase 1 — Core engine + "Listen to PC on phone"

| Item | Status | Notes |
|---|---|---|
| `sp-protocol` (messages, media header, framing, versions) | ✅ | protocol 1.0–1.1 (capability-gated session tickets, network-test probes, TCP stream frames); 20 tests |
| `sp-security` (identity, QR proof, SAS, encrypted trust store) | ✅ | 16 tests |
| `sp-transport` (QUIC, TLS over TCP, mutual TLS, pinning, datagrams, exporter, migration) | ✅ | real QUIC and TCP tests, including a client address change mid-session |
| `sp-discovery` (mDNS, signed beacons, registry, ranking) | 🟡 | unit-tested; multicast needs real-network check |
| `sp-media` (Opus/PCM, adaptive jitter buffer, drift, DSP, RNNoise) | ✅ | 19 tests incl. 1-hour drift simulation |
| `sp-audio-io` (trait, cpal backend, null backend, converters) | 🟡 | unit-tested; device paths need hardware |
| `sp-engine` (actor, sessions, pairing, routes, permissions, reconnect, settings, state) | ✅ | unit tests plus two-engine tests: pairing → audio → prompt → stop → forget, restart-reconnect, resume without a second prompt, shared encoder, live codec switch, USB (TCP) + network test, resume after restart. Audio devices open off the actor |
| Desktop app (Tauri shell, tray, autostart, sleep inhibit, diagnostics, UI) | 🟡 | builds and launches on macOS (identity in Keychain, listening on UDP 47650); svelte-check clean, 9 UI tests |
| Linux desktop: devices and system audio (output monitors) through PipeWire/PulseAudio, deb/rpm/AppImage | 🟡 | `sp-audio-io` `pulse` feature, tested against PulseAudio in a container; packages build (`linux-build.yml`). "Mute PC speakers": PipeWire mutes the default sink; PulseAudio (whose monitors follow the sink mute) moves playback to a temporary "SoundPush Speakers" null sink and restores the previous default on unmute or the next start. Real-server tests in `sp-audio-io/tests/pulse_server.rs` (PulseAudio 16 in a container: captured peak 0.5 while the speakers' monitor is silent) |
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
| Windows stage 2: own "SoundPush Microphone" driver in the repo, built + test-signed in CI | 🟡 | `sound-push-desktop/drivers/windows-virtual-audio` (PortCls/WaveRT: "SoundPush Microphone Feed" → "SoundPush Microphone"); x64 builds locally with `/W4 /WX /analyze`, infverif, inf2cat and ApiValidator clean; CI builds x64 + ARM64 and test-signs (`windows-driver.yml`). Not yet installed on a test-signing PC. Not used by the app: VB-CABLE stays active until attestation signing |
| Windows stage 2: Microsoft attestation signing of the driver | ⛔ | needs an EV code-signing cert + Partner Center account (AudioRelay's driver is signed this way) |
| Linux virtual microphone created by the app (PipeWire/PulseAudio null sink + remap source) | 🟡 | `virtual_mic.rs`; tested against PulseAudio in a container (load, audio, unload) and PipeWire (load, drop-in restore); PipeWire audio needs a desktop session. See [virtual-microphone.md](virtual-microphone.md) |
| Push-to-talk / mute global hotkey | 🟡 | `src-tauri/src/hotkeys.rs` (global-shortcut plugin), recorder on the Audio page, conflict errors, tray check mark follows the engine's `micMuted`; mute also silences the phone mic feeding the virtual mic |
| Auto-start phone mic when an app opens the virtual mic | 🟡 | `desktop.autoStartMic`; `PlatformHooks::virtual_mic_in_use` (Windows: active sessions on "CABLE Output"; macOS: `DeviceIsRunningSomewhere`; Linux: recording streams on the SoundPush source); engine `actor/local_audio.rs` starts the last mic phone and stops it 15 s after the app lets go |

## Phase 3 — Parity completion

| Item | Status | Notes |
|---|---|---|
| Android app audio → PC (playback capture) | 🟡 | `AppAudioCapture` |
| PC mic → phone, PC↔PC, phone↔phone | 🟡 | same route model; untested on hardware |
| Multi-device streaming | ✅ / 🟡 | encoder groups: one capture and encoder per (source, profile) shared by every route that can use it (engine test: two receivers, one capture); needs a multi-phone hardware check |
| Per-stream volume, balance, mono, A/V offset, remote volume/mute, "Mute PC" | ✅ / 🟡 | engine + UI; OS mute via platform calls |
| Adaptive bitrate from receiver stats | ✅ | |
| Packet redundancy (auto on >1 % loss) | ✅ | test recovers frames with 20 % simulated loss |
| Session timer, quality badge, connection details | ✅ | |
| Custom bitrate steps incl. AudioRelay's | ✅ | |
| USB tethering | 🟡 | works as IP network |
| USB via ADB (TLS-over-TCP transport) | 🟡 | ADR-0005: `sp-transport/src/tcp.rs`; desktop listens on loopback, Devices page finds adb and runs `adb reverse`; the phone dials its loopback candidate after 2 s of network candidates. Tested engine to engine over TCP; needs a real phone |
| Session resume after a network interruption | ✅ / 🟡 | single-use resume tokens bound to the peer key (10 min) + QUIC migration; an approved "Ask" route resumes without a second prompt (engine test); needs a Wi-Fi roam check |
| Per-device audio profiles (latency, quality, bitrate, redundancy) | ✅ / 🟡 | `settings.deviceProfiles`, applied at route start and live (bitrate/redundancy live, codec changes renegotiated); desktop Devices page, Android device sheet |
| Network self-test (RTT, jitter, loss, achievable bitrate, recommendation) | ✅ / 🟡 | probes over the media path, ~10 s, cancellable; "Use recommended settings" applies a device profile |
| Local crash reports for Rust panics | ✅ | redacted report in the data folder, notice once on next start, included in the desktop diagnostics export; nothing uploaded |
| Windows per-app capture | 🟡 | `sp-audio-io/src/wasapi_process.rs` (process loopback, one app or everything except one app, Windows 10 2004+); app picker on the Audio page |
| Linux app capture (PipeWire/PulseAudio) | 🟡 | `sp-audio-io/src/pulse.rs` (`AppRouting`): the app's streams (or every other app's) move to a private null sink recorded from its monitor; a loopback keeps them audible on the speakers (~30 ms later); streams opened while capturing follow; streams go back on stop, and after a crash on the next start. App picker on the Audio page (`application.process.binary`). Tested against PulseAudio 16 in a container (only / everything except / late streams / crash cleanup); PipeWire needs a desktop session |
| Audio focus (pause/duck/mix), headphone-unplug pause, auto-resume | 🟡 | Android service |
| Quick Settings tile, reboot reminder, OEM battery guide link | 🟡 | |
| Android "output audio effects" / compatibility output | ⏳ | needs non-cpal playback path |
| Media Feature Pack detection (Windows N) | 🟡 | `src-tauri/src/system.rs` (missing `mfplat.dll`), Home banner → Optional features |
| Windows Firewall / Public network fix | 🟡 | `src-tauri/src/network.rs`: firewall policy + network category read as a normal user; "Allow SoundPush" runs `netsh` through UAC (UDP, this exe, private/domain; public only if chosen); the uninstaller removes the rule with one UAC prompt (`windows/hooks.nsh`) |
| Audio device changes (follow default, device lost) | 🟡 | `src-tauri/src/device_watch.rs` (`IMMNotificationClient`, Core Audio listeners, sound server events on Linux via `pulse::watch_devices`, reconnecting when the server restarts) → `EngineHandle::audio_devices_changed`; routes on the default device reopen, pinned devices report lost |
| Audio cues | 🟡 | `src-tauri/src/cues.rs`, generated tones, `settings.audioCues` |
| Diagnostics export, guided troubleshooter, logs | ✅ (desktop) / 🟡 (Android tips only) | desktop troubleshooter runs checks (firewall, network profile, driver, devices, permissions, Bluetooth) with fix buttons |
| macOS permissions (microphone, System Audio Recording) | 🟡 | `src-tauri/src/macos.rs`: status without prompting, deep links to System Settings. Not yet compiled on macOS |
| Windows ARM64 installer | 🟡 | `windows-build.yml` matrix, artifact `SoundPush-Windows-arm64` (cross-compiled, not yet run) |
| i18n infrastructure (English; locale files, plurals, Intl formats, RTL, pseudo-locales en-XA/ar-XB) | ✅ | `docs/translating.md`; translations via community later |
| Accessibility pass (desktop: keyboard, focus, dialogs, live regions, reduced motion, high contrast, text zoom) | 🟡 | done in code; screen-reader test on NVDA/VoiceOver/Orca and external audit pending |
| Help/About (user guide, privacy, license, report a problem, logs) | ✅ desktop / ✅ Android | |

## Phase 4 — macOS parity

| Item | Status | Notes |
|---|---|---|
| Desktop app runs on macOS (speaker, mic, pairing) | 🟡 | runs on this Mac; phone paired and played audio through the Mac speakers |
| System-audio capture via process taps | 🟡 | `sp-audio-io/src/macos_tap.rs`: tap + aggregate device open and deliver frames; real audio needs the System Audio Recording permission, which macOS grants only to the bundled `SoundPush.app` (unbundled dev binaries receive silence). |
| macOS 13 system audio via ScreenCaptureKit | 🟡 | `sp-audio-io/src/macos_sck.rs`, used before 14.2 (Screen Recording permission; CoreAudio weakly linked in `build.rs` so the tap functions may be missing). Minimum macOS 13. Type-checked and linted for aarch64-apple-darwin only; not yet run on a Mac |
| Login item via `SMAppService` (macOS 13+) | 🟡 | `src-tauri/src/macos.rs`: registers the main app; the old LaunchAgent is removed on the next start; a launch within 2 min of console login counts as autostart; "requires approval" shows the Settings warning. Windows/Linux keep `tauri-plugin-autostart`. Type-checked only |
| AudioServerPlugIn "SoundPush Microphone" | 🟡 | own C driver in `sound-push-desktop/drivers/macos-virtual-mic`, embedded in the app and installed from the Audio page; needs a real install test. Public distribution needs Apple Developer ID notarization |

## Phase 5 — Hardening & release

| Item | Status |
|---|---|
| Real-device matrix, 24 h soak, battery measurement | ⏳ (needs devices); in-process soak and latency harness: `tools/soak` |
| External security review | ⏳ |
| Release workflow: NSIS, universal DMG, deb/rpm/AppImage, APK, SHA256SUMS, CycloneDX SBOMs, draft GitHub Release | 🟡 (`release.yml`, not yet run on GitHub) |
| Desktop auto-update signed with a free minisign key; Android update check against GitHub Releases | 🟡 (needs the `TAURI_SIGNING_*` secrets and a first published release) |
| Paid platform signing (Authenticode, Developer ID + notarisation, Android release key), store listings | ⛔ (certificates/accounts; steps in `docs/release-signing.md`) |
