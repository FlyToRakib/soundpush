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

## Using SoundPush

- Download: [GitHub Releases](https://github.com/FlyToRakib/soundpush/releases/latest) (with `SHA256SUMS`)
- [User guide](docs/user-guide.md): install, pairing, every task, virtual microphone per OS, USB, troubleshooting
- [Privacy policy](PRIVACY.md) · [Changelog](CHANGELOG.md) · [Security policy](SECURITY.md)

## Contributing

[`CONTRIBUTING.md`](CONTRIBUTING.md), [Code of Conduct](CODE_OF_CONDUCT.md), [architecture decisions](docs/adr/README.md),
[threat model](docs/security/threat-model.md), [translating](docs/translating.md),
[release signing](docs/release-signing.md).

## Building

```bash
just core-test      # build and test the shared Rust core
just desktop-dev    # run the desktop app in development mode
just mobile-build   # build the Android app (debug)
```

Requirements: Rust (see `rust-toolchain.toml`), Node 22 + npm, JDK 17, Android SDK + NDK, `cargo-ndk`.

### Windows desktop app (build on Windows)

The same steps run in GitHub Actions (`.github/workflows/windows-build.yml`).

1. Install the prerequisites (once):
   - [Git](https://git-scm.com/download/win)
   - [Rust](https://rustup.rs) (the default MSVC toolchain)
   - [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with **Desktop development with C++**
   - [Node.js 22](https://nodejs.org)
   - WebView2 is already part of Windows 10/11
2. Build (PowerShell):

   ```powershell
   git clone https://github.com/FlyToRakib/soundpush.git
   cd soundpush
   npm --prefix sound-push-desktop/ui ci
   cd sound-push-desktop
   .\ui\node_modules\.bin\tauri build --bundles nsis
   ```

3. The installer is at `target\release\bundle\nsis\SoundPush_0.1.0_x64-setup.exe` (repository root `target` folder).
4. For development with live reload, run `.\ui\node_modules\.bin\tauri dev` from `sound-push-desktop`.

Virtual microphone on Windows: open **Audio → Install VB-CABLE** in the app. See [`docs/virtual-microphone.md`](docs/virtual-microphone.md).

### macOS desktop app

Easiest: GitHub → **Actions** → **macOS build** → latest run → artifact `SoundPush-macOS` (a `.dmg`,
universal for Apple Silicon and Intel). The app is not notarised yet, so macOS shows
*"SoundPush" Not Opened* the first time. Click **Done** (not Move to Trash), then either:

- System Settings → Privacy & Security → **Open Anyway**, or
- in Terminal: `xattr -dr com.apple.quarantine /Applications/SoundPush.app`

If an older build kept asking for the microphone, reset its permission once after installing the
new one: `tccutil reset Microphone net.soundpush.desktop`.

To build on the Mac itself:

1. Install the prerequisites (once): Xcode Command Line Tools (`xcode-select --install`),
   [Rust](https://rustup.rs), [Node.js 22](https://nodejs.org).
2. Build:

   ```bash
   git clone https://github.com/FlyToRakib/soundpush.git
   cd soundpush
   npm --prefix sound-push-desktop/ui ci
   cd sound-push-desktop
   ./ui/node_modules/.bin/tauri build --bundles dmg
   ```

3. The disk image is at `target/release/bundle/dmg/`. The virtual microphone is built in:
   **Audio → Install SoundPush Microphone** (macOS asks for your password).

## License

GPL-3.0-or-later. See `LICENSE`.
