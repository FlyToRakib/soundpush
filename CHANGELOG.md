# Changelog

All notable changes to SoundPush are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Desktop and Android releases share one version number.

Before tagging a release, rename **Unreleased** to the version and date (for example `## [0.1.0] - 2026-10-01`).
The release workflow copies that section into the GitHub Release notes.

## [Unreleased]

First public preview. Nothing has been released yet; everything below is new.

### Added

- **Shared Rust core** (`sound-push-core`): wire protocol, Ed25519 device identity, QR and code pairing with an
  encrypted trust store, QUIC transport with key-pinned mutual TLS, mDNS and signed-beacon discovery, Opus and PCM
  codecs, adaptive jitter buffer, clock-drift compensation, packet redundancy, adaptive bitrate, RNNoise noise
  suppression, and the session/route engine with reconnection and per-device permissions.
- **Desktop app** for Windows, macOS and Linux (Tauri 2 + Svelte 5): tray icon, launch at sign-in, single instance,
  onboarding, Home tasks (listen on phone, phone as microphone, headset, phone audio on the computer), Devices with
  per-device permissions, Audio settings, diagnostics export and logs.
- **Android app** (Kotlin + Jetpack Compose): QR scanning and code pairing, streaming foreground service with
  notification controls, Quick Settings tile, microphone modes and effects, app-audio capture (Android 10+),
  audio focus handling.
- **Multi-device streaming**: always asks which device to use when more than one is connected; live capability updates.
  A source feeds up to eight devices at once (one to sixteen in Settings → Advanced), with an estimate of the bandwidth
  and CPU another listener would cost.
- **Audio + microphone in one stream**: send this computer's sound together with your voice, each with its own level —
  commentary over a game or a film.
- **Connection type** in Settings → Advanced: automatic (the default), Wi-Fi/Ethernet only, TCP only for networks that
  block the fast protocol, or USB only.
- **Error codes**: every message carries a stable code such as `SP-NET-004`, listed with its fix on the new
  [error codes](docs/error-codes.md) page and included in diagnostics exports and logs.
- **Virtual microphone**: built-in "SoundPush Microphone" driver on macOS; one-click VB-CABLE install from the Audio page
  on Windows; only real virtual cables are used, never a speaker.
- **Automatic updates on desktop**: signed update manifests (minisign key, no paid certificate), "Check for updates"
  and "Check for updates automatically" in Settings → About, an update banner with download progress and
  restart-to-install.
- **Update check on Android**: Settings → About shows when a newer release is on GitHub and links to it.
- **Help and About**: user guide, privacy policy, license, source code and "Report a problem" links; logs folder.
- **Accessibility**: visible keyboard focus everywhere, Ctrl+1–4 (⌘1–4) page shortcuts, text zoom with Ctrl +/−,
  screen-reader announcements when streams start, reconnect or stop, labelled meters and progress bars, dialogs that
  keep focus and return it, reduced-motion and Windows high-contrast support.
- **Localisation readiness**: one JSON file per language, plural forms, number and list formatting, right-to-left
  layouts, pseudo-locales for testing (`en-XA`, `ar-XB`), and a translation guide (`docs/translating.md`).
- **Release infrastructure**: release workflow for Windows (NSIS, x64 and ARM64), macOS (universal DMG), Linux (deb,
  rpm, AppImage) and Android (APK) with checksums, SBOMs and a draft GitHub Release; dependency license and advisory
  checks in CI.
- **Update channels**: Stable and Beta in Settings → About. Stable updates reach installs in stages (10 % → 50 % →
  100 % over three days, can be held or halted); Beta gets `x.y.z-beta.n` versions as soon as they are published.
- **Store manifests**: winget, Homebrew cask, Flathub, F-Droid and Google Play listings, filled in from each release
  (`packaging/`, `tools/release/fill-manifests.mjs`).
- **Documentation site** (MkDocs Material on GitHub Pages) with the user guide, troubleshooting, virtual microphone,
  privacy, translating and release pages; the app's help links open it, or GitHub when it can't be reached.
- **Translations on Weblate**: component setup for the desktop and Android strings (`docs/translating.md`).
- **Quality gates in CI**: clippy with warnings as errors, ktlint for the Android modules and ESLint with Prettier for
  the desktop UI, a line coverage report with an 80 % gate on the core crates, SHA-256 verification of every Android
  dependency, the audio backends against PipeWire and WASAPI, nightly fuzzing of every fuzz target, an Android startup
  benchmark that runs nightly and on demand, and a hook for desktop end-to-end tests.
- **Developer tools**: `tools/netsim` applies the impairment profiles the simulation tests use to a real network
  interface, and `tools/latency-probe` measures end-to-end pipeline latency from a chirp. Both build on the new
  `sp-testkit` crate.
- **Documentation**: user guide, privacy policy, threat model, release-signing guide, UX flows and copy rules,
  ADRs 0006–0020, issue and pull request templates.
- **Resume streams after restart**: the streams that were running start themselves again once the device is back.
  Off by default; Settings → Privacy & security on the computer, Settings → Background on the phone.
- **"Another app is using the microphone"**: while Android hands SoundPush silence because a call or an assistant has
  taken the microphone, the phone says so on Home and in the notification, and clears it when they let go.
- **Noise suppression where you want it**: RNNoise runs on the phone or on the computer, so you choose which
  device spends the CPU. It also switches itself off (with a notice) when a machine stops keeping up, and comes
  back when it recovers.
- **Resilient streaming**: Auto, Always or Off, instead of only automatic. Auto stays the default.
- **Test tools on the desktop Audio page**: a test tone on the chosen output, and a microphone test you can hear.
- **Clip indicator** on the level meters, announced as well as coloured, when the volume boost hits the limiter.
- **Feedback-loop warning** while monitoring the microphone on speakers, with a headphones suggestion and an
  optional "Reduce echo on speakers" for desktops without echo cancellation (ADR-0020).
- **"Replace current microphone source?"** when a second device asks to be a computer’s microphone.
- **Android**: balance, the 80 Hz low-cut filter, and a “Recommended” badge on the microphone mode list.
- **Keep window in memory for instant reopen** (Settings → General, off): closing the window normally frees the memory
  it used; keep it loaded to reopen instantly.
- **Make SoundPush the default input and output while active** (Settings → General, off): while your phone is in use,
  apps pick it up without being set up one by one, and your usual devices come back when the stream stops.
- **Real-time audio priority** for capture and playback, so a busy computer causes fewer dropouts: MMCSS "Pro Audio"
  on Windows, the thread time-constraint policy on macOS, `SCHED_RR` on Linux. Fails softly where it is not allowed.
- **Crash reports for crashes a Rust panic hook cannot see** (an access violation, a fault inside an audio driver):
  a minidump and a short note, stored on your computer next to the existing reports. Nothing is uploaded, and the
  diagnostics export still carries only the text.
- **WebView2 check on Windows**: a missing or damaged runtime is found before the window is built, and SoundPush
  offers Microsoft's installer instead of showing a blank window. Streaming and the tray keep working meanwhile.
- **Troubleshooter**: a VPN that carries your whole connection, a network that blocks devices from talking to each
  other, and audio-enhancement software that is running and is known to break recording.
- **Advanced settings** (Settings → Help & support): continuous capture, keep audio devices open and real-time audio
  priority, folded away and explained in [`docs/advanced.md`](docs/advanced.md).
- **Flatpak and Wayland**: global shortcuts, launch at sign-in and "keep the computer awake" go through
  `xdg-desktop-portal`, which is the only way they can work there.

### Changed

- Windows virtual microphone plan: bundled VB-CABLE first, SoundPush's own signed driver later (ADR-0003, ADR-0007).
- Engine notices close by themselves; messages stay open while hovered or focused.
- Connecting now tries a device's addresses at the same time, 250 ms apart, instead of one after another: a device
  that answers on its second address no longer waits out the first one's timeout.
- Lossless audio drops to Opus 256 kb/s by itself when the connection keeps losing packets, and goes back to lossless
  once it is stable again. Both changes are announced.
- Android debug builds share one signing key so builds from any machine update each other. Release builds are signed
  by Gradle from the release keystore when CI has one, and produce a Play App Bundle alongside the APK.
- Android battery: with the app in the background or the screen off, stream statistics and level meters stop being
  decoded and drawn. Streams, the notification and reconnection are unaffected.
- "Auto" quality on the phone now means uncompressed audio on a charger over a good link, and Opus on battery.

### Fixed

- Windows firewall fix: on a network Windows calls public, "Allow SoundPush" used to add a rule for private networks
  only, spend the administrator prompt and leave the phone just as blocked. It now offers to move that network to the
  Private profile, or to allow SoundPush on public networks, and does either in the same single prompt.
- The "Windows Firewall blocks other devices" notice no longer stays up while a paired device is connected.
- Windows firewall fix, hardened: the system directory now comes from Windows itself rather than an environment
  variable, the tools it runs are named by their full path, and the elevated step starts in the system folder — so
  nothing on the computer can decide what the administrator prompt actually runs.
- Update checks stay out of the way: the first one waits half a minute when Windows, macOS or Linux started SoundPush
  at sign-in, and a metered connection is left alone. "Check for updates" always works.
- Audio devices no longer stay claimed after the last stream: what SoundPush keeps between streams is released a
  minute later, and the phone-microphone check no longer polls when no connected device could supply one.
- Windows build: dual-stack UDP socket and COM feature flags.
- Repeated handshake failures and stuck dials between engines.
- Desktop: the engine stops cleanly on quit (speakers unmuted, peers told), and start failures are shown instead of
  "Starting…" forever.
- macOS: the microphone permission prompt no longer repeats endlessly (signed app with the audio-input entitlement).
- Windows uninstaller removes SoundPush's data when "Delete the application data" is ticked.
- Android: computer-initiated app-audio streams ask for screen-capture consent instead of sending silence.
- Pairing by address: "Back" no longer submits the form.
- CI: the Android job builds the Kotlin bindings again (the host build needed the ALSA development package).

[Unreleased]: https://github.com/FlyToRakib/soundpush/commits/HEAD
