# SoundPush

Free and open-source audio routing between your computer and phone.

- Hear your **PC audio on your phone**
- Use your **phone as a PC microphone**
- Use your **phone as a wireless headset**
- Send **phone app audio** to a PC or another phone
- Stream to **multiple devices**, over Wi-Fi or USB

Secure pairing, end-to-end encrypted, no accounts, no ads, no telemetry.

## Status: early preview (0.x)

SoundPush works, and it is young. Releases before 1.0 are published as **pre-releases**: expect rough edges,
and please [report what you find](https://github.com/FlyToRakib/soundpush/issues/new/choose).

| Platform | State |
|---|---|
| Windows 11 (x64) | Tested on a real PC with a phone: pairing, listening, phone as microphone (through VB-CABLE) and headset. Windows 10 is supported but untested |
| Android 8+ | Tested on a real phone (Xiaomi, Android 12) and by automated UI tests; other phones and Android versions are untested — reports welcome |
| Linux (x64) | Builds and passes automated tests against real PipeWire and PulseAudio; **not yet used by a person — help wanted** |
| macOS 13+ | Builds in CI; **never run on a Mac — help wanted** |
| Windows on ARM64 | Builds in CI; **untested — help wanted** |

The installers are not code-signed yet, so Windows and macOS warn the first time; the
[user guide](docs/user-guide.md#1-install) shows how to continue. Known limitations are listed in the
[changelog](CHANGELOG.md).

## Download and install

1. Open **[Releases](https://github.com/FlyToRakib/soundpush/releases)** and download the file for your system:
   `…_x64-setup.exe` (Windows), `…_android.apk` (Android), `.deb`/`.rpm`/`.AppImage` (Linux), `.dmg` (macOS).
2. Install SoundPush on your computer **and** your phone, on the same Wi-Fi (or connected by USB).
3. Open both, choose **Pair** on the computer and scan the QR code with the phone.
4. Pick a task, for example **Listen to computer**.

Step-by-step instructions, the virtual microphone and troubleshooting: [user guide](docs/user-guide.md).

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

- Download: [GitHub Releases](https://github.com/FlyToRakib/soundpush/releases) (with `SHA256SUMS`)
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

Desktop UI checks (run in `sound-push-desktop/ui`): `npm run check` (svelte-check), `npm test` (Vitest unit tests) and `npm run test:e2e` (Playwright: the built UI against the in-browser mock engine in light and dark themes, with axe WCAG 2.1 AA checks on every page and dialog; install the browser once with `npx playwright install chromium`).

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
universal for Apple Silicon and Intel; macOS 13 or later, where macOS 13 to 14.1 record system audio
through Screen Recording). The app is not notarised yet, so macOS shows
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

### Linux desktop app

Easiest: GitHub → **Actions** → **Linux build** → latest run → artifact `SoundPush-Linux` (x86_64):

- Ubuntu, Debian, Linux Mint, Pop!_OS: `sudo apt install ./SoundPush_0.1.0_amd64.deb`
- Fedora, openSUSE: `sudo dnf install ./SoundPush-0.1.0-1.x86_64.rpm`
- Other distributions: `chmod +x SoundPush_0.1.0_amd64.AppImage`, then run it.

SoundPush uses PipeWire (the default on current Ubuntu and Fedora) or PulseAudio; on both it can
send one app's sound (or everything except one app) and mute the speakers while sending. The packages
are built on the GitHub `ubuntu-latest` runner, so they need a distribution at least as new as
its glibc. The tray icon needs AppIndicator support; on plain GNOME install the
"AppIndicator and KStatusNotifierItem Support" extension (Ubuntu ships it).

To build on Linux (Ubuntu/Debian):

1. Install the prerequisites (once): [Rust](https://rustup.rs), [Node.js 22](https://nodejs.org), and

   ```bash
   sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
     libasound2-dev libpulse-dev libdbus-1-dev pkg-config build-essential patchelf file xdg-utils
   ```

2. Build:

   ```bash
   git clone https://github.com/FlyToRakib/soundpush.git
   cd soundpush
   npm --prefix sound-push-desktop/ui ci
   cd sound-push-desktop
   ./ui/node_modules/.bin/tauri build --bundles deb,rpm,appimage
   ```

3. The packages are in `target/release/bundle/deb/`, `rpm/` and `appimage/`.

Virtual microphone on Linux: **Audio → Install SoundPush Microphone**. No driver or password is
needed; in Discord, Zoom or Meet choose **SoundPush Microphone**. See
[`docs/virtual-microphone.md`](docs/virtual-microphone.md).

## License

GPL-3.0-or-later. See `LICENSE`.
