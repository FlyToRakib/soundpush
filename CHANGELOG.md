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
- **Quality gates in CI**: clippy with warnings as errors, line coverage report with an 80 % gate on the core
  crates, nightly fuzzing of every fuzz target, and a hook for desktop end-to-end tests.
- **Documentation**: user guide, privacy policy, threat model, release-signing guide, ADRs 0006–0019, issue and pull
  request templates.

### Changed

- Windows virtual microphone plan: bundled VB-CABLE first, SoundPush's own signed driver later (ADR-0003, ADR-0007).
- Engine notices close by themselves; messages stay open while hovered or focused.
- Android debug builds share one signing key so builds from any machine update each other.

### Fixed

- Windows firewall fix: on a network Windows calls public, "Allow SoundPush" used to add a rule for private networks
  only, spend the administrator prompt and leave the phone just as blocked. It now offers to move that network to the
  Private profile, or to allow SoundPush on public networks, and does either in the same single prompt.
- The "Windows Firewall blocks other devices" notice no longer stays up while a paired device is connected.
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
