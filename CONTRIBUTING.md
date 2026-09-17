# Contributing to SoundPush

Thanks for helping! SoundPush is free and open source (GPL-3.0-or-later).

## Where help is most useful right now

SoundPush is an early preview, so the most valuable contribution needs no code at all:

- **Try it on a system nobody has tested yet** — Linux (any distribution, PipeWire or PulseAudio), macOS 13+,
  Windows on ARM64, Windows 10, and Android phones other than Xiaomi — and
  [report how it went](https://github.com/FlyToRakib/soundpush/issues/new/choose), good or bad.
- **Report bugs** with the diagnostics report attached (see below).
- **Translate** the apps into your language: [docs/translating.md](docs/translating.md).
- **Improve the docs** where something was unclear to you: [docs/user-guide.md](docs/user-guide.md).

## Getting started with the code

1. Read the [architecture decisions](docs/adr/README.md); [`docs/soundpush-final.md`](docs/soundpush-final.md) is the
   full plan.
2. Install Rust (see `rust-toolchain.toml`), Node 22, JDK 17, and the Android SDK/NDK plus `cargo-ndk` for mobile work.
   The [README](README.md#building) has the build steps per platform.
3. Before opening a pull request run the checks for what you changed. With [`just`](https://github.com/casey/just):
   `just core-test`, `just desktop-check`, `just mobile-build`. Without it, the same commands are in the `justfile`.
   CI runs all of them on every pull request.

## Guidelines

- **Keep it simple.** Prefer small, focused changes. No new dependency without a clear reason.
- **UI:** minimal and consistent. Use design tokens (`design/tokens`), support Light/Dark/System,
  label every control for screen readers, and never convey state by color alone.
- **Rust:** `cargo fmt`, `cargo clippy` clean. No `unwrap()` outside tests. Network-facing parsers
  must never panic on bad input and should have tests (and a fuzz target where practical).
- **Kotlin:** `./gradlew ktlintCheck lint` clean. **Desktop UI:** `npm run lint` and `npm run check` clean.
- **Security-sensitive areas** (`sp-security`, `sp-transport`, pairing, the driver) need two reviewers.
- **Big changes** start with a short ADR in `docs/adr/`.
- **Commit messages** name the area and say what changed for the user, for example
  `Android: a stream that starts is scrolled into view on Home`, with a body that explains why.
  Sign off your commits (`git commit -s`).

## Reporting bugs

Open an issue with steps to reproduce. Attach the diagnostics report from
Settings → Help & diagnostics → Export diagnostics (review it first; it contains no audio).
On Android, `adb logcat -s SoundPush` adds the phone's side.

Security issues: see [`SECURITY.md`](SECURITY.md).
