# Contributing to SoundPush

Thanks for helping! SoundPush is free and open source (GPL-3.0-or-later).

## Getting started

1. Read [`docs/soundpush-final.md`](docs/soundpush-final.md) for the architecture.
2. Install Rust (see `rust-toolchain.toml`), Node 22, JDK 17, and the Android SDK/NDK for mobile work.
3. Run `just core-test` and `just desktop-check` before opening a pull request.

## Guidelines

- **Keep it simple.** Prefer small, focused changes. No new dependency without a clear reason.
- **UI:** minimal and consistent. Use design tokens (`design/tokens`), support Light/Dark/System,
  label every control for screen readers, and never convey state by color alone.
- **Rust:** `cargo fmt`, `cargo clippy` clean. No `unwrap()` outside tests. Network-facing parsers
  must never panic on bad input and should have tests (and a fuzz target where practical).
- **Security-sensitive areas** (`sp-security`, `sp-transport`, pairing, the driver) need two reviewers.
- **Big changes** start with a short ADR in `docs/adr/`.
- Use [Conventional Commits](https://www.conventionalcommits.org/) and sign off your commits (`git commit -s`).

## Reporting bugs

Open an issue with steps to reproduce. Attach the diagnostics report from
Settings → Help & diagnostics → Export diagnostics (review it first; it contains no audio).

Security issues: see [`SECURITY.md`](SECURITY.md).
