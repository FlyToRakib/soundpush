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
| CI quality gates: fmt, clippy `-D warnings`, tests, svelte-check, vitest, `cargo deny`, `npm audit --audit-level=high`, Android unit tests + lint, coverage, release-tool tests, desktop e2e hook | 🟡 | `ci.yml`; Android job fixed (host `libasound2-dev` for the binding build). Coverage (`cargo-llvm-cov`): core crates 92 % lines, gated at 80 %; sp-engine 74 %, reported only. e2e job runs `npm run test:e2e` once it exists. Needs a green run on GitHub |
| Fuzz targets (media, control, framing, QR, beacon) + nightly fuzzing | ✅ | `fuzz/` — `cargo +nightly fuzz run <target>`; `fuzz.yml` runs each target 30 min nightly, corpus cached, crashes uploaded |
| Documentation site (MkDocs Material, GitHub Pages) | 🟡 | `mkdocs.yml`, `docs.yml`; builds with `--strict`. Needs Pages enabled (Settings → Pages → GitHub Actions) and a push to `main` |
| Community translation (Weblate) | 🟡 | component settings in `docs/translating.md`, `.weblate`; needs the Hosted Weblate project created |

## Phase 1 — Core engine + "Listen to PC on phone"

| Item | Status | Notes |
|---|---|---|
| `sp-protocol` (messages, media header, framing, versions) | ✅ | protocol 1.0–1.1 (capability-gated session tickets, network-test probes, TCP stream frames); 20 tests |
| `sp-security` (identity, QR proof, SAS, encrypted trust store) | ✅ | 16 tests |
| `sp-transport` (QUIC, TLS over TCP, mutual TLS, pinning, datagrams, exporter, migration) | ✅ | real QUIC and TCP tests, including a client address change mid-session |
| `sp-discovery` (mDNS, signed beacons, registry, ranking) | 🟡 | unit-tested; multicast needs real-network check |
| `sp-media` (Opus/PCM, adaptive jitter buffer, drift, DSP, RNNoise) | ✅ | 19 tests incl. 1-hour drift simulation |
| `sp-audio-io` (trait, cpal backend, null backend, converters) | 🟡 | unit-tested; device paths need hardware |
| `sp-engine` (actor, sessions, pairing, routes, permissions, reconnect, settings, state) | ✅ | unit tests plus two-engine tests: pairing → audio → prompt → stop → forget, restart-reconnect, resume without a second prompt, shared encoder, live codec switch, USB (TCP) + network test, resume after restart, anti-flap fallback to Stable, security log, pairing rate limit. Audio devices open off the actor. `#![forbid(unsafe_code)]` |
| Connection `Degraded` state (§19.1) and anti-flap (§20) | ✅ | `health.rs`: loss > 3 % or jitter > 30 ms for 3 s, back after 5 s below 1 % / 15 ms; > 5 drops in 2 min holds the device on Stable for 10 min with a notice |
| DTX during silence (§15.2) | ✅ | Opus only, `FEATURE_DTX` capability bit 31; header-only DTX packets after 200 ms hangover, keep-alive every 400 ms; receiver plays comfort silence without loss or underruns |
| DSCP EF marking (§16.3) | 🟡 | `sp-transport/src/qos.rs`: qWAVE flows on Windows; `IP_TOS`/`IPV6_TCLASS` elsewhere (TCP carries it; QUIC packets not yet, quinn-udp 0.5 sends ECN-only TOS per packet). Needs a packet capture on real networks |
| Security log (§21.1, §28.2) and pairing rate limit (5/min per address) | ✅ | `audit.rs` (1000 entries / 30 days, local only), `pairing_limit.rs`, `StopReason::RateLimited`; viewers in desktop and Android Settings |
| Logging (§28.1) | ✅ / 🟡 | `logging.rs`: size rotation 5 × 10 MB desktop, 3 × 2 MB Android; "Detailed logs" setting raises the level to debug and switches off after 24 h; Kotlin logs go through `log_message` FFI into the same files and logcat; Android diagnostics include the log files |
| Performance benches (§22.2) | ✅ | `cargo bench -p sp-media`: Opus encode 10 ms stereo 128 kb/s ≈ 101 µs (budget 300), decode ≈ 26 µs, RNNoise ≈ 94 µs (200), mic DSP chain ≈ 6 µs, jitter push+pop ≈ 0.15 µs, drift resample ≈ 4 µs (x64 dev PC). Budgets in `benches/media.rs` |
| Simulated network (§29.1) | ✅ | `sp-engine/tests/sim_network.rs`: real sender/receiver pipelines through a seeded link with loss + redundancy, 40 ms jitter/reordering, duplication, 800 ms blackout; ~4 s |
| Settings schema migrations (§29.1) | ✅ | `settings.rs` `migrate` (raw JSON, one step per version, written back); version 2 |
| Desktop app (Tauri shell, tray, autostart, sleep inhibit, diagnostics, UI) | 🟡 | builds and launches on macOS (identity in Keychain, listening on UDP 47650); svelte-check clean, Vitest units, Playwright e2e against the mock engine in light and dark with axe WCAG 2.1 AA checks (`npm run test:e2e`) |
| Desktop window lifecycle (plan §13.1, §24) | 🟡 | `window_state.rs`: size, position and maximized state restored onto a connected monitor; the window waits up to 1.5 s for the engine; autostart opens it only when "Start in the background" is off or onboarding is unfinished; one-time "SoundPush keeps running" hint on the first close to the tray |
| Desktop tray (plan §13.2, §23.1) | 🟡 | `tray.rs`: per-route Mute/Stop, microphone mute with shortcut, recent devices (connect, listen, use as microphone), status badge (streaming, microphone live, attention, error); Linux without a StatusNotifier host minimizes instead of hiding |
| OS network change, sleep/resume and session end → engine | 🟡 | `os_events.rs`: Windows `NotifyIpInterfaceChange` + hidden window (`WM_POWERBROADCAST`, `WM_ENDSESSION`) + `RegisterApplicationRestart` (Restart Manager); macOS IOKit power + SystemConfiguration keys (not yet compiled on macOS); Linux rtnetlink + logind `PrepareForSleep`. Needs a real sleep/Wi-Fi-switch check on each OS |
| Linux desktop: devices and system audio (output monitors) through PipeWire/PulseAudio, deb/rpm/AppImage | 🟡 | `sp-audio-io` `pulse` feature, tested against PulseAudio in a container; packages build (`linux-build.yml`). "Mute PC speakers": PipeWire mutes the default sink; PulseAudio (whose monitors follow the sink mute) moves playback to a temporary "SoundPush Speakers" null sink and restores the previous default on unmute or the next start. Real-server tests in `sp-audio-io/tests/pulse_server.rs` (PulseAudio 16 in a container: captured peak 0.5 while the speakers' monitor is silent) |
| Android app (engine FFI, service, notifications, tile, QR scan, screens) | 🟡 | debug APK builds (arm64); needs a real phone to test. Notification Stop / Mute / Output (system output switcher per Android version), widget Mute, denied/blocked permission states with Open settings, process-level network watcher, per-app language picker (generated locale list, pseudo-locales in debug), custom latency, output device choice (platform player), connection details, OSS licences (AboutLibraries), share app, tablet/foldable layouts (rail, list-detail Devices, two-column Home). Robolectric flow tests with ATF accessibility checks and Light/Dark/RTL/en-XA/tablet screenshots |

## Phase 2 — Microphone & headset

| Item | Status | Notes |
|---|---|---|
| Phone mic → PC virtual mic (compatibility mode: VB-CABLE/BlackHole/Voicemeeter) | 🟡 | ADR-0003 |
| Android mic presets (7 modes) + platform AEC/NS/AGC with availability | 🟡 | Kotlin `MicCapture` |
| Gain 0–20 dB, soft limiter, RNNoise, level meter, mic monitor | ✅ | The limiter's clip indicator is on the meter on both platforms, announced as well as coloured (§8.3) |
| "Ask" permission prompt for microphone | ✅ | tested end-to-end |
| Headset mode (both routes in one action) | 🟡 | desktop + Android home task; on Android the recorder uses the voice-call preset with echo cancellation while the headset task runs and returns to the user's preset afterwards. RNNoise now runs on the phone **or** the computer, the user's choice (§4.3/§15.7). The desktop has no AEC: [ADR-0020](adr/0020-desktop-echo-control.md) explains why and what it does instead |
| Noise suppression on the phone or on the PC (§4.3, §15.7) | ✅ | `mic.noiseSuppressionAt`; the microphone's device decides per route and carries it in `StreamProfile.denoise`, gated by `FEATURE_RECEIVER_DENOISE` so 1.0 peers fall back to sender-side denoising. Receiver-side RNNoise in `pipeline/receiver.rs`; desktop and Android both expose it |
| RNNoise auto-disable when capture misses deadlines (§8.3) | ✅ | `sp-engine/src/denoise.rs` supervises capture overruns: off after a burst with one notice, on again after 20 clean seconds. The setting is never rewritten, so it is on again at the next start |
| Microphone feedback-loop detection (§8.2) | ✅ | `sp_media::dsp::FeedbackDetector` in the monitor path: a loud tone that holds its pitch while it builds is a howl; speech and room noise are not (unit-tested both ways). Warns once and points at headphones or Headset mode |
| Desktop echo control without an AEC (§15.7) | ✅ | [ADR-0020](adr/0020-desktop-echo-control.md): no C/C++ AEC fits the CI toolchains, so `mic.echoDucking` ducks the microphone 18 dB while this device plays the far side on its speakers (`DuckGate`, far-end level from a shared `EchoReference`). Off by default; headphones stay the recommendation |
| Two routes want the same virtual microphone (§8.2) | ✅ | The engine allows one feed and refuses a second with `error.audio.virtualMicBusy` (locally and over the wire as `DeviceBusy`); desktop and Android then ask "Replace current microphone source?" and retry with `replace`. Engine test with two phones |
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
| Per-stream volume, balance, mono, A/V offset, remote volume/mute, "Mute PC" | ✅ / 🟡 | engine + UI; OS mute via platform calls. `PeerView.speakersMuted` shows the state; a peer's remote mute ends with its session (engine test). Desktop: sending mutes this computer's speakers, receiving from another computer mutes that one |
| Adaptive bitrate from receiver stats | ✅ | |
| Packet redundancy ("Resilient": Auto / Always / Off) | ✅ | Auto is the unchanged default (on above 1 % loss, off after 10 clean seconds); Always and Off are the user's call (§15.6). Settings schema 3 migrates the old boolean, globally and per device. Test recovers frames with 20 % simulated loss |
| Session timer, quality badge, connection details | ✅ | desktop: transport and path (USB/adb, tethering, local, IPv4/IPv6), address, per-stage latency (capture, encode, network, buffer, output from `RouteStats`), 5-minute latency/loss chart with a text summary |
| Custom bitrate steps incl. AudioRelay's | ✅ | |
| USB tethering | 🟡 | works as IP network; desktop `tethering.rs` detects RNDIS/NCM/Apple tethering adapters, labels devices reached through them and warns when the computer's internet uses the phone's mobile data (Windows/Linux unit-tested parsers; macOS reader not yet compiled) |
| USB via ADB (TLS-over-TCP transport) | 🟡 | ADR-0005: `sp-transport/src/tcp.rs`; desktop listens on loopback, Devices page finds adb and runs `adb reverse`; the phone dials its loopback candidate after 2 s of network candidates. Tested engine to engine over TCP; needs a real phone |
| Session resume after a network interruption | ✅ / 🟡 | single-use resume tokens bound to the peer key (10 min) + QUIC migration; an approved "Ask" route resumes without a second prompt (engine test); needs a Wi-Fi roam check |
| Per-device audio profiles (latency, quality, bitrate, redundancy) | ✅ / 🟡 | `settings.deviceProfiles`, applied at route start and live (bitrate/redundancy live, codec changes renegotiated); desktop Devices page, Android device sheet |
| Desktop test tools: test tone and microphone test (§5.1) | ✅ | Audio page; the tone reuses the cue generator on the chosen output, the microphone test borrows the engine's monitor for 15 s without touching the saved "Listen to my microphone" setting |
| Android parity: balance, 80 Hz low-cut, "Recommended" mic-mode badge (§4.3, §23.1) | ✅ | Audio screen; same semantics and wording as the desktop, with TalkBack labels and ATF-checked Robolectric tests |
| Network self-test (RTT, jitter, loss, achievable bitrate, recommendation) | ✅ / 🟡 | probes over the media path, ~10 s, cancellable; "Use recommended settings" applies a device profile |
| Local crash reports for Rust panics | ✅ | redacted report in the data folder, notice once on next start, included in the desktop diagnostics export; nothing uploaded |
| Windows per-app capture | 🟡 | `sp-audio-io/src/wasapi_process.rs` (process loopback, one app or everything except one app, Windows 10 2004+); app picker on the Audio page |
| Linux app capture (PipeWire/PulseAudio) | 🟡 | `sp-audio-io/src/pulse.rs` (`AppRouting`): the app's streams (or every other app's) move to a private null sink recorded from its monitor; a loopback keeps them audible on the speakers (~30 ms later); streams opened while capturing follow; streams go back on stop, and after a crash on the next start. App picker on the Audio page (`application.process.binary`). Tested against PulseAudio 16 in a container (only / everything except / late streams / crash cleanup); PipeWire needs a desktop session |
| Audio focus (pause/duck/mix), headphone-unplug pause, auto-resume | 🟡 | Android service; "Lower volume" uses a separate output duck gain (`set_output_duck`), route volumes are never overwritten |
| Quick Settings tile, reboot reminder, OEM battery guide link | 🟡 | |
| Android "output audio effects" / compatibility output | ⏳ | needs non-cpal playback path |
| Media Feature Pack detection (Windows N) | 🟡 | `src-tauri/src/system.rs` (missing `mfplat.dll`), Home banner → Optional features |
| Windows Firewall / Public network fix | 🟡 | `src-tauri/src/network.rs`: firewall policy + network category read as a normal user; "Allow SoundPush" runs `netsh` through UAC (UDP, this exe, private/domain; public only if chosen). On a public network the dialog will not run until the user picks what to do about it, and the same elevated step moves that network to the Private profile (`Set-NetConnectionProfile`, adapters from `INetworkConnection::GetAdapterId`) when they say it is their own. The Home notice stays quiet while a device is connected; the uninstaller removes the rule with one UAC prompt (`windows/hooks.nsh`) |
| Audio device changes (follow default, device lost) | 🟡 | `src-tauri/src/device_watch.rs` (`IMMNotificationClient`, Core Audio listeners, sound server events on Linux via `pulse::watch_devices`, reconnecting when the server restarts) → `EngineHandle::audio_devices_changed`; routes on the default device reopen, pinned devices report lost |
| Audio cues | 🟡 | `src-tauri/src/cues.rs`, generated tones, `settings.audioCues` |
| Diagnostics export, guided troubleshooter, logs | ✅ (desktop) / 🟡 (Android tips only) | desktop troubleshooter runs checks (firewall, network profile, driver, devices, permissions, Bluetooth) with fix buttons; the export is previewed first (sections, what is removed, exact text) and redacts addresses, device ids and the pairing code, also inside log lines. The "Detailed logging" toggle appears once the engine exposes `debugLogging` |
| macOS permissions (microphone, System Audio Recording) | 🟡 | `src-tauri/src/macos.rs`: status without prompting, deep links to System Settings. Not yet compiled on macOS |
| Windows ARM64 installer | 🟡 | `windows-build.yml` matrix, artifact `SoundPush-Windows-arm64` (cross-compiled; build passes on GitHub); also built and published by `release.yml` with a `windows-aarch64` updater entry. Needs a test on an ARM64 PC |
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
| Release workflow: NSIS (x64 + ARM64), universal DMG, deb/rpm/AppImage, APK, SHA256SUMS, CycloneDX SBOMs, draft GitHub Release, beta pre-releases | 🟡 (`release.yml`, not yet run on GitHub) |
| Desktop auto-update signed with a free minisign key; Android update check against GitHub Releases | 🟡 (needs the `TAURI_SIGNING_*` secrets and a first published release) |
| Update channels (stable/beta) and staged rollout 10 % → 50 % → 100 % over 72 h, hold/halt | 🟡 (`settings.updateChannel`, `check_update`, `rollout.ts`, `update-channels.yml` + `tools/release/channels.mjs`; unit-tested; needs a first published release) |
| Store manifests: winget, Homebrew cask, Flathub, F-Droid, Play listing texts | 🟡 (`packaging/`, fastlane metadata, filled per release by `fill-manifests.mjs`; winget validated, AppStream validated. Submissions need store accounts; Flathub needs screenshots and portal-based autostart/sleep inhibit) |
| Paid platform signing (Authenticode, Developer ID + notarisation, Android release key) | ⛔ (certificates/accounts; steps in `docs/release-signing.md`) |
