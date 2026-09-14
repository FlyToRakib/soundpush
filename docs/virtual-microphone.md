# Virtual microphone ("Use phone as computer microphone")

This document explains how the phone's microphone reaches other apps (Google Meet,
Zoom, Discord, OBS, recorders) on each desktop platform. It covers what is built today,
how AudioRelay solves the same problem, and what is still needed on Windows.

## 1. Why a virtual microphone is needed

Apps such as Meet or a recorder can only choose a microphone from the list the operating
system provides. A normal app like SoundPush cannot add a device to that list. Only an
**audio driver** can.

The flow is the same everywhere:

```
Phone mic ──(encrypted QUIC stream)──► SoundPush engine
                                        │ plays the audio into the driver's *feed* side
                                        ▼
                               Virtual microphone driver
                                        │ exposes it as a normal *microphone*
                                        ▼
                          Meet / Zoom / Discord / recorder
```

The engine never plays the phone microphone through the real speakers. If the chosen
device is not a virtual cable, SoundPush ignores it and shows a warning. See
`sound-push-desktop/src-tauri/src/hooks.rs` → `VIRTUAL_CABLES` / `cable_input_name`.

## 2. How AudioRelay does it (research, September 2026)

| Platform | AudioRelay | Source |
|---|---|---|
| Windows 10/11 | Ships its own driver, **"Virtual Mic for AudioRelay"** (`audiorelay.virtual-mic.inf`, MEDIA class). It is installed by its installer through a helper (`AudioConnect.DriverHelper.exe`). The driver is **signed by "Microsoft Windows Hardware Compatibility Publisher"**. They paid for an EV code-signing certificate and submitted the driver to Microsoft for attestation signing. | community.audiorelay.net/t/virtual-mic-driver-does-not-install/1482 |
| Windows 7 | Tells users to install VB-CABLE. | audiorelay.net/docs/windows/use-your-phone-as-a-mic-for-windows-7 |
| macOS | **No driver of its own.** The docs tell users to install BlackHole, VB-Cable, GroundControl or Loopback. | audiorelay.net/docs/macos/virtual-audio-devices |

So AudioRelay has no free trick on Windows. Its driver loads on normal PCs only because
Microsoft signed it.

## 3. What SoundPush does today

### macOS: built in, no third-party software ✅

- Driver source: `sound-push-desktop/drivers/macos-virtual-mic/SoundPushMicrophone.c`.
  It is our own CoreAudio **AudioServerPlugIn** in C, commented line by line.
  - One device, **"SoundPush Microphone"**, with an output stream (SoundPush writes the
    phone audio here) and an input stream (apps record from here). A ring buffer on the
    device clock connects the two.
  - The output side can't become the default speaker, so it never shows up as a speaker
    in System Settings. When nothing feeds it, apps read silence.
- Build: `sound-push-desktop/drivers/macos-virtual-mic/build.sh`. It runs automatically
  from `src-tauri/build.rs` and makes a universal arm64 + x86_64, ad-hoc signed bundle.
- Bundling: `src-tauri/tauri.macos.conf.json` copies `SoundPushMicrophone.driver` into
  `SoundPush.app/Contents/Resources`.
- Install and remove: `src-tauri/src/virtual_mic.rs`.
  1. Copies the bundle to `/Library/Audio/Plug-Ins/HAL`.
  2. Restarts `coreaudiod`.
  3. Runs through the standard macOS administrator prompt, so SoundPush never sees the password.
- UI: Audio page → Virtual microphone → **Install SoundPush Microphone**.
- Distribution note: for users who download the app, SoundPush.app, including the
  driver, must be signed with an Apple Developer ID and notarized ($99/year). Local
  builds work as they are.

### Windows: VB-CABLE installed together with SoundPush (stage 1, next to build) 🟡

- **One installer.** The SoundPush Windows installer contains the official VB-CABLE
  package. It is downloaded from vb-audio.com during the GitHub build and never committed
  to the repo. The installer installs VB-CABLE silently in the same run, so the user sees
  one installer, one UAC prompt and one restart request at the end. VB-CABLE needs a
  reboot after its first install.
- **Skips what exists.** If VB-CABLE is already installed, the step is skipped.
  Uninstalling SoundPush leaves VB-CABLE in place, because other apps may use it.
- **Credit (required by VB-Audio's licence).** The installer and the Audio page show
  "VB-CABLE by VB-Audio. VB-CABLE is a donationware, all participations are welcome",
  with a link to vb-audio.com. Only the standard VB-CABLE may be bundled, not A+B or C+D.
- **No engine change needed.** SoundPush already detects "CABLE Input" and feeds the
  phone microphone into it. Apps choose **"CABLE Output"**.
- **Available now, until VB-Audio agrees to bundling:** Audio page → **Install VB-CABLE**.
  SoundPush downloads the official, SHA-256-pinned package, opens VB-Audio's own setup
  (UAC, then "Install Driver") and offers **Restart now** (`src-tauri/src/virtual_mic.rs`).
  The user never visits a website or unzips anything.

### Linux: built in, no driver 🟡

- **No driver, no root, no password.** SoundPush talks to the sound server with the PulseAudio
  protocol. PipeWire serves it through pipewire-pulse, so this works on PipeWire desktops
  (current Ubuntu, Fedora) and on PulseAudio-only systems. It loads two modules:
  1. `module-null-sink` **"SoundPush Microphone Feed"**: the engine plays the phone microphone into it.
  2. `module-remap-source` **"SoundPush Microphone"**, reading that sink's monitor: apps choose
     it as their microphone.
- **Keeping it.** Loaded modules last until the sound server stops (logout, reboot, a restart of
  PipeWire). SoundPush keeps the device in two ways:
  - A marker file (`~/.local/share/SoundPush/virtual-microphone`) makes every SoundPush start
    create it again. SoundPush retries for a minute because it can start before the sound server at login.
  - On PipeWire, SoundPush also writes `~/.config/pipewire/pipewire-pulse.conf.d/soundpush-microphone.conf`,
    so pipewire-pulse creates the device at login before SoundPush starts, and apps keep it
    selected. PulseAudio has no per-user drop-in short of replacing `default.pa`, so there it
    depends on SoundPush starting (turn on **Launch at login**).
  - If the device is gone anyway, the Audio page says so; **Install SoundPush Microphone** brings it back.
- **Remove:** Audio → **Remove SoundPush Microphone** unloads both modules and deletes the marker
  and the drop-in.
- **Detection:** "SoundPush Microphone Feed" is the playback side, "SoundPush Microphone" the
  recording side (`VIRTUAL_CABLES`). SoundPush hides the feed from its own speaker lists, but
  desktop sound settings show it as an output; don't choose it as your speakers.
- **Other SoundPush devices.** Sending one app's sound adds a temporary "SoundPush App Capture"
  output, and muting the speakers on PulseAudio a "SoundPush Speakers" output. They are not
  microphones: SoundPush hides them and removes them when it is done (or on its next start).
- **In use:** `virtual_mic::virtual_mic_in_use()` tells whether an app is recording from the
  source (pavucontrol's and GNOME Settings' level meters don't count). It is not wired into the
  engine yet.
- **Tested:** in a container with PulseAudio 16 (modules load, a tone played into the feed comes
  out of SoundPush Microphone, modules unload), and PipeWire 0.3.65 (modules load, the drop-in
  brings them back after pipewire-pulse restarts). Audio through PipeWire still needs a real
  desktop session to verify.

Testing on Linux:

1. Install SoundPush (GitHub → Actions → "Linux build" → artifact `SoundPush-Linux`).
2. SoundPush → Audio → Virtual microphone → **Install SoundPush Microphone**. It should say
   *Ready… choose "SoundPush Microphone"*. In a terminal, `pactl list short sources` lists `soundpush_microphone`.
3. Phone → **Use as computer microphone** → pick the PC.
4. Discord, Zoom or Meet → microphone → **SoundPush Microphone**.

## 4. Windows: what a built-in driver requires

Windows 10/11 (64-bit, Secure Boot on) loads a kernel audio driver only if it is signed by
Microsoft. The only free-of-charge route Microsoft offers, **attestation signing**, still
needs an **EV code-signing certificate** registered in the Partner Center (about
$250–$500 per year).

Options we checked:

| Option | Works on normal PCs? | Cost | Notes |
|---|---|---|---|
| Own driver + attestation signing (what AudioRelay does) | ✅ | EV cert, about $250–$500 per year | Best end result; needs a certificate owner |
| Own driver, test-signed | ❌ Only with `bcdedit /set testsigning on` | Free | Fine for development on your own PC, not for users |
| SignPath Foundation (free for open source) | ❌ for kernel drivers | Free | OV signing only, which Windows does not accept for kernel drivers |
| Open-source "Virtual-Audio-Driver" (VirtualDrivers) | ❌ Needs test mode | Free | Its README tells users to enable test signing |
| Bundle VB-CABLE | ✅ | Licence from VB-Audio | Still third-party; redistribution needs their permission |
| User installs VB-CABLE (current) | ✅ | Free | One-time install by the user |

**Final plan for Windows (decided 2026-09-13):**

1. **Stage 1: bundle VB-CABLE (now).** VB-CABLE is installed together with SoundPush
   (§3). It is free and Microsoft-signed.
   **Blocked until VB-Audio agrees in writing.** Their licensing web page allows bundling,
   but the licence inside the package (`readme.txt`) says: *"It is not allowed to integrate
   the VB-CABLE package in another software installation procedure without Author
   agreement."* Ask through https://vb-audio.com/Services/contact.htm. The download
   script `sound-push-desktop/drivers/vbcable/fetch.sh` (SHA-256 pinned) is ready; the
   installer step is added only after the agreement arrives.
2. **Stage 2: our own driver (in parallel, switched on after signing).**
   - **Built (not switched on):** `sound-push-desktop/drivers/windows-virtual-audio/` is a
     WaveRT/PortCls driver. It has a playback endpoint, "SoundPush Microphone Feed", that the
     engine plays into, and a capture endpoint, "SoundPush Microphone", that apps record from.
     A lock-free ring buffer on a shared clock joins them. Architecture, build and test
     install are in its `README.md`.
   - **Built and test-signed in GitHub Actions** (`.github/workflows/windows-driver.yml`,
     artifact `SoundPush-Windows-Driver-TestSigned`, x64 + ARM64). Developers can test it with
     `bcdedit /set testsigning on`. It has not yet been installed on a test PC.
   - **VB-CABLE stays the active driver** until attestation signing (postponed together with
     code signing). The app has no install UI for this driver.
   - When the project has an EV certificate and a Microsoft Partner Center account,
     submit the driver for attestation signing.
   - From then on the installer installs "SoundPush Microphone" instead of VB-CABLE.
   - The engine already prefers "SoundPush Microphone" over "CABLE Input" (`VIRTUAL_CABLES`
     order in `hooks.rs`), so users who still have VB-CABLE keep working.

There is always exactly **one** virtual microphone that SoundPush installs: VB-CABLE
before the signed driver exists, and SoundPush Microphone after.

## 5. Testing on Windows today

1. Install SoundPush (GitHub → Actions → "Windows build" → artifact `SoundPush-Windows-x64`).
2. SoundPush → Audio → Virtual microphone → **Install VB-CABLE**. Accept the Windows prompt,
   click **Install Driver** in VB-Audio's setup window, close it, then click **Restart now**.
3. SoundPush → Audio → Virtual microphone. It should say
   *Ready… choose "CABLE Output (VB-Audio Virtual Cable)"*. If it warns that a speaker was
   chosen, click **Use automatic**.
4. Phone → **Use as computer microphone** → pick the PC.
5. Meet → Settings → Audio → Microphone → **CABLE Output**. Windows Sound Recorder uses
   the default input: Settings → System → Sound → Input → CABLE Output.

## 6. Code map

| What | Where |
|---|---|
| Which devices count as a virtual microphone | `sound-push-desktop/src-tauri/src/hooks.rs` (`VIRTUAL_CABLES`) |
| Engine capability (`virtualMic`, `virtualMicDevice`, `virtualMicInput`) | `sound-push-core/sp-engine/src/actor.rs` → `local_capabilities` |
| Capabilities re-announced to the phone when they change | `actor.rs` → `tick` (mid-session `Hello`) |
| macOS driver | `sound-push-desktop/drivers/macos-virtual-mic/` |
| Windows driver (test-signed, not used yet) | `sound-push-desktop/drivers/windows-virtual-audio/`, `.github/workflows/windows-driver.yml` |
| Linux sound server client (devices, streams, modules) | `sound-push-core/sp-audio-io/src/pulse.rs` |
| Install / remove / status commands | `sound-push-desktop/src-tauri/src/virtual_mic.rs`, `commands.rs` |
| Audio page UI | `sound-push-desktop/ui/src/features/Audio.svelte` |
