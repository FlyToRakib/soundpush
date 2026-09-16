# Implementation status

Where SoundPush stands against [the plan](soundpush-final.md). Every line below was checked against the
code in this repository on 16 September 2026, and names the file that proves it. Nothing here is taken
from the commit history or from an earlier version of this page.

**How to read the statuses**

| Status | Meaning |
|---|---|
| Done | Written and covered by automated tests. Where it has never run on real hardware, the note says so. |
| Partial | Some of what the plan asks for is in place; the note says what is missing. |
| Not started | No code yet. |
| Postponed | A deliberate decision — see [Deliberately postponed](#deliberately-postponed). |
| Needs a person | The code is finished; someone has to do something outside the repository — see [Needs a person, not code](#needs-a-person-not-code). |

---

## What works today, in plain words

**Everything is built and tested by machine. Nothing has yet been used on a phone, a Mac, an ARM64 PC
or a real Wi-Fi network by a person.** That is the single biggest caveat on this page, and it applies
to every platform below.

**Windows** is the furthest along. The desktop app builds as an NSIS installer for x64 and ARM64
(`windows-build.yml`), and its WASAPI paths — system audio, per-application capture, device
enumeration — are exercised against the real audio stack by `sp-audio-io/tests/wasapi_smoke.rs`.
Everything else the plan asks of Windows is written: receiving a phone's audio, feeding a phone's
microphone into a virtual cable, the tray, global hotkeys, the firewall and network-profile helper,
the WebView2 and Media Feature Pack checks, crash reports and diagnostics. The virtual microphone is
still VB-CABLE; our own driver exists and is test-signed in CI but the app does not use it.

**Linux** has the same desktop app. System audio, per-application capture, speaker muting and the
app-created virtual microphone all run against a real PipeWire daemon in CI
(`.github/workflows/ci.yml`, "Audio backend"), and deb, rpm and AppImage packages build. The XDG
portal paths used inside Flatpak and on Wayland compile but have never run in a real session.

**macOS** builds. CI produces a universal `.dmg` and checks its signature and audio-input entitlement
(`.github/workflows/macos-build.yml`), so all the macOS-only code — process taps, ScreenCaptureKit,
`SMAppService`, the Core Audio device watcher, the TCC permission flows — genuinely compiles for the
platform rather than being type-checked. **Nobody has run this build on a Mac.** The bundled
"SoundPush Microphone" AudioServerPlugIn has never been installed.

**Android** builds a debug APK and a Play App Bundle, and passes its unit and Robolectric flow tests
with accessibility checks (`app/src/test/.../MainFlowsTest.kt`) and Light/Dark/RTL/tablet screenshots
(`ScreenshotTest.kt`). It has the streaming service, notification and Quick Settings controls, the
widget, pairing by QR and code, microphone modes, app-audio capture, the audio screen, a
troubleshooter that runs real checks, and the security log.
**No physical phone has run it** — the development phone has never enumerated over USB.

**The shared engine** is the most thoroughly tested part: protocol, pairing and trust, QUIC and
TLS-over-TCP transports, discovery, Opus and PCM, the jitter buffer, drift compensation, redundancy,
the route engine, reconnection and settings, with two-engine integration tests that pair, stream,
interrupt and resume. Core line coverage is gated at 80 % in CI.

---

## Status by plan section

### §4 Feature parity matrix

| Plan section | What the plan asked for | Status | Evidence / note |
|---|---|---|---|
| §4.1 Streaming capabilities | Every source and sink combination: PC audio → phone, mic → any device, phone app audio → PC, phone ↔ phone, multiple receivers, Wi-Fi, USB tethering, ADB, headset mode | Done | Symmetric route model in `sp-engine/src/actor.rs`; `AppAudioCapture.kt`; ADB over `sp-transport/src/tcp.rs`; `HeadsetMode.kt`. No hardware test. |
| §4.2 Connection and devices | Discovery, connect by address, connections list, secure pairing, per-device permissions, remote route control | Done | `sp-discovery/src/{mdns,beacon,registry,candidates}.rs`, `sp-security`, `features/Devices.svelte`, `feature-devices`. Multicast has never been tried on a real network. |
| §4.3 Audio settings | Codec and bitrate, latency profiles, output effects, exclusive audio, mute PC, gain and RNNoise, mic presets, device selection | Done | `sp-engine/src/settings.rs`, `features/Audio.svelte`, `feature-audio/.../AudioScreen.kt`. Compatibility output and output audio effects are implemented, not pending — see §4.6. |
| §4.4 App, settings and support | Stats, theme, language, startup, update check, crash reports, logs, troubleshooter, share links | Done | `features/Settings.svelte`, `feature-settings/.../SettingsScreen.kt`, `sp-engine/src/crash.rs`. |
| §4.5 Platform parity | Windows, Linux, macOS and Android at the same level for 1.0 | Partial | Windows, Linux and Android are built and tested by machine; macOS builds in CI but has never been run (`macos-build.yml`). |
| §4.6 Re-audit additions | Session timer, remote Mute PC and volume, mic monitoring, **output audio effects toggle**, exact bitrate steps, dismissible tips, Media Feature Pack and missing-device detection | Done | Output effects and compatibility output: `platform-service/.../PlatformPlayback.kt` (AudioTrack path, `ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION`), wired in `StreamingService.kt:233` `syncPlatformPlayback`, settings at `AudioScreen.kt:183-191`. Tips: `features/PlatformBanners.svelte`, `settings.dismissedTips`. |

### §5–§8 Improvements and edge cases

| Plan section | What the plan asked for | Status | Evidence / note |
|---|---|---|---|
| §5.1–§5.7 Improvements over AudioRelay | Headset mode, per-app capture, free premium features, pairing, accessibility, easy setup | Done | One deviation: the desktop has no acoustic echo canceller. [ADR-0020](adr/0020-desktop-echo-control.md) explains why and what replaces it. |
| §8.1 Network edge cases | Loss, blackouts, lossless fallback, transport choice | Done | `sp-engine/src/health.rs` `QualityFallback` (PCM → Opus 256 kb/s after 5 s above 2 % loss, back after 15 s below 0.5 %); `settings.transport` pin (Auto/QUIC/TCP/USB). The TCP pin has never met a genuinely UDP-blocked network. |
| §8.2 Device and session mistakes | Two routes wanting one virtual microphone, feedback loops | Done | `error.rs:138` `error.audio.virtualMicBusy`, with a "Replace current microphone source?" prompt and retry (`features/Home.svelte:42`, `SoundPush.kt:139`); `sp_media::dsp::FeedbackDetector` in the monitor path. |
| §8.3 Audio edge cases | Clipping, RNNoise cost, microphone taken by another app | Done | Clip indicator: `pipeline/sender.rs:436-440` → `state.rs:294` → `LevelMeter.svelte` (announced as well as coloured), and `MainFlowsTest.kt:240` on Android. RNNoise auto-disable on capture overruns: `sp-engine/src/denoise.rs`. Mic-in-use notice: `DeviceStatus.kt:151` (`isClientSilenced`) → banner in `MainActivity.kt:701`. |
| §8.4 Lifecycle and platform | Crashes, restarts, low memory | Done | `sp-engine/src/crash.rs` (redacted text report) plus `src-tauri/src/crash_dump.rs` (minidump for faults the panic hook never sees); Android `Caches.kt` with `onTrimMemory`, and `StateUpdates.kt` throttling snapshots while the app is hidden. |

### §10–§14 Architecture

| Plan section | What the plan asked for | Status | Evidence / note |
|---|---|---|---|
| §10 Technology stack | Shared Rust core, Tauri 2 + Svelte desktop, Kotlin + Compose Android, QUIC, Opus + PCM | Done | ADRs 0001, 0004, 0005, 0017, 0018. The plan named Oboe for Android audio; the code uses cpal, which uses AAudio there — recorded in [ADR-0002](adr/0002-audio-backends.md). |
| §10.7, §13.4 Windows virtual microphone | Bundled VB-CABLE first, own signed driver next | Postponed | Stage 1 waits on the VB-Audio agreement; stage 2 is built and test-signed (`drivers/windows-virtual-audio`, `windows-driver.yml`) but the app still uses VB-CABLE. |
| §11 Monorepo | Structure, workspaces, `justfile`, dependency rules | Done | `Cargo.toml`, `justfile`, `deny.toml` (dependency direction is a `cargo deny` check). |
| §12 Shared core | `sp-protocol`, `sp-security`, `sp-transport`, `sp-discovery`, `sp-media`, `sp-audio-io`, `sp-engine`, `sp-testkit` | Done | All eight crates exist under `sound-push-core/`. `sp-engine` is `#![forbid(unsafe_code)]`. |
| §13.1–§13.2 Desktop modules | Tray, window lifecycle, hotkeys, crash reports, portals | Done | `src-tauri/src/` — `tray.rs`, `window_state.rs` (including `desktop.keepWindowInMemory`, `settings.rs:342`), `hotkeys.rs`, `crash_dump.rs`, `portals.rs`. |
| §13.3 Desktop audio I/O | Device selection, follow default, per-app capture, real-time priority | Done | `sp-audio-io/src/{cpal_backend,wasapi_process,pulse,macos_tap,macos_sck}.rs`; `src-tauri/src/device_watch.rs`; `sp-audio-io/src/rt_priority.rs` (MMCSS, Mach time-constraint policy, `SCHED_RR`). Priority has not been load-tested on any OS. |
| §13.5 Background operation | Close to tray, idle behaviour | Done | `CloseHint.svelte`; `actor/local_audio.rs` `IdleAudio` closes the devices kept between streams after 60 s. |
| §13.6 Permissions and system integration | Firewall rule, network profile, WebView2, Media Feature Pack | Done | `src-tauri/src/network.rs` (elevated `netsh`, absolute system paths, refusal of odd program paths), `webview2.rs`, `system.rs`. The plan's separate signed helper is deliberately not built; the reasoning is in [advanced.md](advanced.md). |
| §14 Mobile architecture | Service, UI, audio, discovery, background, battery | Done | `platform-service/`, the `feature-*` modules, `core-engine/`. Battery behaviour has never been measured on a phone. |

### §15–§20 Audio and networking

| Plan section | What the plan asked for | Status | Evidence / note |
|---|---|---|---|
| §15.1–§15.3 Wire format and pipelines | 48 kHz, Opus/PCM, send and receive pipelines, mixed source | Done | `sp-engine/src/pipeline/{sender,receiver,monitor}.rs`; system audio and microphone mixed with independent gains via `Capabilities::SOURCE_MIXED` (`sp-protocol/src/version.rs:82`). The two capture clocks have never met real hardware. |
| §15.2 DTX | Silence detection on Opus | Done | `FEATURE_DTX` capability bit 31; header-only packets after a 200 ms hangover, keep-alive every 400 ms. |
| §15.4–§15.5 Latency profiles and drift | Low latency / Balanced / Stable / Custom, adaptive buffer, drift compensation | Done | `sp-media`, including a one-hour drift simulation test. |
| §15.6 Loss resilience | Redundancy Auto / Always / Off | Done | The "Resilient" control exists in both apps: `features/Audio.svelte:234` and `AudioScreen.kt:116-129`, with a per-device override at `features/Devices.svelte:281`. The settings schema migrates the old boolean. |
| §15.7 Microphone processing | Gain, limiter, RNNoise on either end, AEC | Done | Gain, soft limiter and clip indicator as in §8.3; `mic.noiseSuppressionAt` chooses sender or receiver (`settings.rs:240`), gated by `FEATURE_RECEIVER_DENOISE` (`version.rs:104`) so 1.0 peers fall back to the sender; receiver-side RNNoise at `pipeline/receiver.rs:210-214`, with a test at `receiver.rs:490-558` that the playout really suppresses hiss. No desktop AEC — `mic.echoDucking` instead, per [ADR-0020](adr/0020-desktop-echo-control.md). |
| §15.9 Latency budget | Capture to output inside the Balanced budget | Partial | Per-stage latency is measured and shown (`RouteStats`), and `tools/latency-probe` measures it end to end from a chirp, but the budget has never been checked on a real link. |
| §16.1–§16.2 Transports and protocol | QUIC, TLS over TCP, mutual TLS, pinning, migration, versioned messages | Done | `sp-transport`; real QUIC and TCP tests, including a client address change mid-session. |
| §16.3 QoS | DSCP EF marking | Partial | `sp-transport/src/qos.rs`: qWAVE on Windows, `IP_TOS`/`IPV6_TCLASS` elsewhere. TCP carries the mark; QUIC does not, because quinn-udp 0.5 sends an ECN-only TOS control message. A test pins the version so the limitation is revisited when the dependency moves. |
| §17 Discovery | mDNS, signed beacons, last-known addresses, candidate ranking and racing | Done | `sp-discovery`; `sp-engine/src/net.rs:79` `race_candidates` — 250 ms stagger, first authenticated handshake wins, losers cancelled, USB keeps a 2 s head start. |
| §18 Pairing | Identity, QR and code, SAS, trust and permissions | Done | `sp-security` (16 tests); a pairing rate limit of 5 per minute per address in `sp-engine/src/pairing_limit.rs`. |
| §19.1 Session state machine | Including a `Degraded` state | Done | `sp-engine/src/health.rs`: degraded above 3 % loss or 30 ms jitter for 3 s, recovered after 5 s below 1 % and 15 ms. |
| §19.2 Multi-device | Encoder sharing and a receiver limit | Done | One capture and encoder per (source, profile); `settings.max_receivers` defaults to 8, range 1–16, refused with `SP-CFG-003`/`DeviceBusy` (`actor.rs:2619-2635`), with the bandwidth and CPU estimate both UIs show first (`safe_receivers`). |
| §20 Reconnection | Backoff, resume tokens, anti-flap | Done | `sp-engine/src/{reconnect,resume}.rs`; more than five drops in two minutes holds the device on Stable for ten minutes. Never tried across a real Wi-Fi roam. |

### §21–§28 Security, performance, UI and diagnostics

| Plan section | What the plan asked for | Status | Evidence / note |
|---|---|---|---|
| §21.1–§21.2 Security controls | Pinned mutual TLS, trust store, consent, security log | Done | `sp-security`; `sp-engine/src/audit.rs` (1000 entries, 30 days, local only) with viewers in both apps. |
| §21 External review | An independent security review before 1.0 | Needs a person | None has been commissioned. |
| §22.1 Performance budgets | Release-blocking budgets for start-up, CPU, memory, size and glitch rate | Partial | Criterion benches cover the media hot paths and document the budget for each (`sp-media/benches/media.rs`), but nothing asserts them, and the application-level budgets have not been measured on any platform. |
| §22.2 Continuous benchmarking | CI tracks regressions over 10 % | Not started | No workflow runs `cargo bench`; `android-benchmark.yml` covers Android start-up only, and it has never had a device. |
| §23.1–§23.5 UI and accessibility | Information architecture, flows, theming, WCAG AA | Done | Desktop: `svelte-check`, Vitest, and Playwright e2e in light and dark with axe WCAG 2.1 AA checks. Android: Robolectric flow tests with ATF checks. No screen-reader pass on NVDA, VoiceOver or Orca. |
| §23.6 Copy and localisation | i18n from day one, community translations | Partial | The machinery is complete — `ui/src/lib/i18n/` with plurals and `Intl` formats, RTL, pseudo-locales `en-XA`/`ar-XB`, and [translating.md](translating.md). **The only locale file is `en.json`**, and Android has no `values-*` language folder. |
| §24 Desktop startup and background | Autostart, start hidden, resume routes, update timing | Done | "Resume streams after restart" exists globally in both apps: `settings.rs:518` `resume_routes_on_start`, with toggles at `features/Settings.svelte:202` and `SettingsScreen.kt:137`. Update timing: `updater.svelte.ts` delays the first check by 30 s after a sign-in launch and leaves metered connections alone. |
| §25 Mobile background | Foreground service, OS constraints, lifecycle | Done | `StreamingService.kt`, `BootReceiver.kt`, `BatteryGuide.kt`. Untested on a phone. |
| §26 Permissions | Android runtime permissions, desktop and macOS TCC, SoundPush-level consent | Done | `src-tauri/src/macos.rs` reads status without prompting and deep-links to System Settings; it compiles for macOS in CI but has never been exercised there. |
| §27 Error handling | A shared taxonomy with stable public codes | Done | `sp-engine/src/error.rs` `code()` and `stop_reason_code()` reach state snapshots, the FFI, both UIs, diagnostics and logs; listed in [error-codes.md](error-codes.md). |
| §28.1 Logging | Rotation 5 × 10 MB desktop, 3 × 2 MB Android; a debug level that reverts after 24 h | Partial | `sp-engine/src/logging.rs` and `settings.rs:625-641` (`schedule_debug_logging`, `expire_debug_logging`, unit-tested). The desktop toggle is live at `features/Settings.svelte:251`. **Android has no Detailed logging toggle** — the setting reaches `EngineState.kt:316` but no screen offers it. |
| §28.2 Diagnostics | Connection details, network test, guided troubleshooter, export, audit log, crash reports | Done | Desktop `features/Troubleshooter.svelte`. **Android runs real checks too**: `feature-settings/.../Troubleshooter.kt` builds network, audio, microphone and background checks from live state with fix buttons (`findChecks`, `audioChecks`, `micChecks`, `backgroundChecks`) — it is not a list of tips. Deviation: the export carries only the redacted text crash report, never the minidump, because a dump holds process memory. |

### §29–§36 Process, build and release

| Plan section | What the plan asked for | Status | Evidence / note |
|---|---|---|---|
| §29.1 Test pyramid | Unit, simulation, fuzz, backend integration, e2e, soak | Done | `sp-testkit` (seeded impairment link, chirp cross-correlation); `sp-engine/tests/sim_network.rs`; `fuzz/` with targets run 30 minutes each nightly (`fuzz.yml`); `tools/{netsim,latency-probe,soak}`. |
| §29.2 Device and OS matrix | A release gate across Windows, macOS, Linux and Android hardware | Not started | Needs a device lab — see below. |
| §29.3 Quality gates | Format, clippy `-D warnings`, ktlint, eslint, coverage, dependency checks | Done | `.github/workflows/ci.yml` runs: `core` (Ubuntu, Windows and macOS — `cargo fmt --check`, the clippy gate through `tools/ci/clippy-gate.mjs`, workspace tests, PulseAudio server tests), `backends` (PipeWire and WASAPI), `coverage` (core gated at 80 % lines, engine reported), `desktop-ui` (eslint, Prettier, svelte-check, Vitest, `npm audit`), `desktop-e2e` (Playwright; the `test:e2e` script now exists, so the job really runs), `release-tools`, `dependencies` (`cargo-deny`) and `android` (assemble, bundle, unit tests, lint, ktlint, SHA-256 dependency verification). Alongside it: `fuzz.yml`, `windows-driver.yml`, `docs.yml`, `android-benchmark.yml`, and the Windows, macOS and Linux build workflows. |
| §30 Standards and Definition of Done | Conventions and a review checklist | Done | `CONTRIBUTING.md`, `clippy.toml`. No `TODO`, `FIXME`, `todo!()` or `unimplemented!()` anywhere in the application code. |
| §31 Dependency strategy | Pinned, audited, verified | Done | `deny.toml`; `android/gradle/verification-metadata.xml` covers 855 components and is regenerated against a cold Gradle home by `just mobile-verification`. |
| §32 Build, release and update | Installers for four platforms, checksums, SBOMs, a signed updater, channels, staged rollout | Partial | `release.yml` builds NSIS x64 and ARM64, a universal DMG, deb/rpm/AppImage, and an Android APK **and App Bundle**, with checksums and SBOMs; `update-channels.yml` with `tools/release/channels.mjs` does stable/beta and the 10 % → 50 % → 100 % rollout. Nothing has been released and the signing secrets are unset. |
| §33.1 Extension points | Designed into v1 | Done | Symmetric route model ([ADR-0009](adr/0009-symmetric-route-model.md)), capability bits, settings migrations (`settings.rs` `migrate`, schema v3). |
| §33.2–§33.3 Roadmap and local API | Post-1.0 | Not started | By design. |
| §36.1 Open-source model | GPL-3.0, no telemetry, everything free | Done | `LICENSE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `PRIVACY.md`, `CHANGELOG.md`, issue and PR templates, CODEOWNERS, Dependabot. |
| §36.3 Documentation deliverables | Protocol, security, ADRs, user guide, troubleshooting, UX docs | Done | `docs/protocol`, `docs/security`, ADRs 0001–0020, [user-guide.md](user-guide.md), [troubleshooting.md](troubleshooting.md), [advanced.md](advanced.md), [error-codes.md](error-codes.md), [virtual-microphone.md](virtual-microphone.md), `ux/flows.md`, `ux/copy.md`. The site builds with `mkdocs build --strict` but is not published. |
| §36.5 Internationalisation | Ready from day one | Partial | See §23.6. |

---

## Deliberately postponed

Two decisions, taken knowingly, and the things they hold up.

**1. Paid code signing and notarisation.** Certificates and developer accounts cost money and the
project is not releasing yet. The build system is ready for them: `release.yml` enables Windows
Authenticode once `WINDOWS_CERTIFICATE` is set and Apple signing and notarisation once
`APPLE_CERTIFICATE` is set, and the steps are written down in [release-signing.md](release-signing.md).
Free signing is used where it exists — the desktop updater uses a minisign key
([ADR-0019](adr/0019-release-signing-without-paid-certificates.md)).

This blocks: installing on Windows without a SmartScreen warning; Gatekeeper accepting the macOS
`.dmg`, which is ad-hoc signed today and needs "Open Anyway"; publishing to Google Play with a real
release key; and Microsoft attestation signing of our Windows driver, which is what item 2 waits for.

**2. Switching Windows from VB-CABLE to our own driver.** The driver is finished and built:
`sound-push-desktop/drivers/windows-virtual-audio` (PortCls/WaveRT, "SoundPush Microphone Feed" →
"SoundPush Microphone"), compiled for x64 and ARM64 and test-signed by `windows-driver.yml` with
infverif, inf2cat and ApiValidator clean. It is **not used by the application**, because an unattested
driver cannot load on an ordinary PC.

This blocks: retiring the VB-CABLE dependency, and with it the need for the VB-Audio bundling
agreement; the optional "SoundPush Speakers" render endpoint (§13.4, §4.6), so "Mute PC speakers" on
Windows mutes the default endpoint (`src-tauri/src/hooks.rs:239`) instead of taking the render-endpoint
route the plan prefers; and any test of the driver on a test-signing PC.

---

## Needs a person, not code

| What | Why it is stuck |
|---|---|
| Enable GitHub Pages | Repository Settings → Pages → Source: "GitHub Actions". `docs.yml` builds and uploads the site every time and skips the deployment with a notice until an administrator does this. |
| `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets | Without them `release.yml` builds no updater bundles, so installed apps would never see an update. The key is free to generate. |
| `LINUX_GPG_PRIVATE_KEY` and `LINUX_GPG_PASSPHRASE` secrets | `SHA256SUMS.asc` is skipped until they are set. Also free. |
| A written agreement with VB-Audio | The VB-CABLE licence requires one before the installer may bundle the package. The pinned download script is ready ([virtual-microphone.md](virtual-microphone.md) §4). |
| A Hosted Weblate project | The component configuration is in [translating.md](translating.md) and `.weblate`; the project itself has to be created. |
| Store accounts | Google Play, Microsoft Partner Center, Flathub, F-Droid and Homebrew submissions. Manifests live in `packaging/` and are filled per release by `tools/release/fill-manifests.mjs`; Flathub also wants screenshots. |
| An external security review | §21 asks for one before 1.0. |
| A device lab and real-network testing | The whole of §29.2, plus a DSCP packet capture, a Wi-Fi roam, an ARM64 Windows PC, a machine without the WebView2 runtime, a Flatpak or Wayland session, a battery measurement, and **running the macOS build on a Mac**. |
| Translations | The i18n machinery is complete; somebody has to write the locale files. There are none in any language but English. |

---

## Known gaps

What is genuinely missing or partial, with the plan section it belongs to.

1. **§16.3 — DSCP on QUIC.** TCP carries the EF mark; QUIC does not, because quinn-udp 0.5 attaches
   only the ECN bits. Blocked upstream. `sp-transport/src/qos.rs`.
2. **§22.1 — Performance budgets unverified.** Start-up time, idle CPU and memory, per-stream CPU, APK
   and installer size, connection time and glitch rate have not been measured on any platform.
3. **§22.2 — No benchmark regression tracking.** `sp-media/benches/media.rs` exists and lists a budget
   per bench, but no workflow runs `cargo bench` and nothing asserts the budgets, so a 10 % regression
   would pass unnoticed. The bench file says as much in its own header comment.
4. **§29.2 — No hardware matrix at all.** Nothing has run on a physical Android phone, a Mac, an ARM64
   Windows PC, or a Wayland or Flatpak Linux session.
5. **§23.5 — No screen-reader pass.** The automated axe and ATF checks pass; NVDA, VoiceOver and Orca
   have not been used, and no external accessibility audit has been done.
6. **§23.6, §36.5 — No translations.** `ui/src/lib/i18n/en.json` is the only locale file, and Android
   has no `values-*` language folder.
7. **§28.1 — Android has no Detailed logging toggle.** The engine setting and its 24-hour expiry are
   implemented and reach `EngineState.kt:316`, but only the desktop exposes it.
8. **§28.2 — Minidumps are not in the diagnostics export.** A deliberate deviation: the export carries
   the redacted text crash report only, because a minidump holds process memory.
9. **§13.4, §4.6 — No "SoundPush Speakers" render endpoint on Windows.** "Mute PC speakers" mutes the
   default endpoint instead. Held up by the postponed driver switch.
10. **§17, §20 — Discovery and roaming untested on real networks.** mDNS multicast, client and AP
    isolation, and QUIC migration across a Wi-Fi roam are covered by unit and simulation tests only.
11. **§13.3 — Real-time audio priority untested under load.** `sp-audio-io/src/rt_priority.rs` fails
    softly on every platform, so a silent failure would go unnoticed without a load test.
12. **Virtual microphone drivers never installed.** Neither the Windows driver (test-signed in CI) nor
    the macOS AudioServerPlugIn (`drivers/macos-virtual-mic`, embedded in the app bundle) has been
    installed on a real machine.
13. **§32 — Nothing released.** The release, channel and store workflows have never run against a real
    tag, so the updater path is unproven end to end.
