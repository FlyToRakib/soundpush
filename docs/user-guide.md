# SoundPush user guide

SoundPush connects your computer and your Android phone so you can:

- **Listen on your phone** to everything your computer plays,
- **Use your phone as the computer's microphone** in Zoom, Meet, Teams, Discord, OBS or any recorder,
- **Use your phone as a headset** (both at once),
- **Hear your phone's apps** on the computer, or send them to another phone,

over Wi-Fi, a phone hotspot or USB. Pairing is encrypted, there is no account, and audio never leaves your devices.

Contents: [Install](#1-install) · [Pair](#2-pair-your-phone-and-computer) · [Tasks](#3-everyday-tasks) ·
[Virtual microphone](#4-virtual-microphone) · [USB](#5-usb) · [Settings](#6-settings) ·
[Keyboard and accessibility](#7-keyboard-and-accessibility) · [Updates](#8-updates) ·
[Troubleshooting](#9-troubleshooting) · [Uninstall](#10-uninstall-and-your-data)

---

## 1. Install

Download from **[GitHub Releases](https://github.com/FlyToRakib/soundpush/releases/latest)**. Each release lists a
`SHA256SUMS` file; to check a download run `sha256sum -c --ignore-missing SHA256SUMS` (Linux/macOS) or
`Get-FileHash .\file` (PowerShell) and compare.

### Windows 10 and 11

1. Run `SoundPush_<version>_x64-setup.exe`. It installs for your user only; no administrator rights needed.
2. Until the installer is code-signed, SmartScreen may say *Windows protected your PC*: choose
   **More info → Run anyway**.
3. If Windows Firewall asks, allow SoundPush on **private networks**.
4. SoundPush starts in the tray (notification area). It opens with Windows unless you turn that off.

### macOS 13 or newer (Apple Silicon and Intel)

1. Open `SoundPush_<version>_universal.dmg` and drag SoundPush to **Applications**.
2. Until builds are notarised, macOS says it can't verify SoundPush the first time. Click **Done**, then
   **System Settings → Privacy & Security → Open Anyway**.
3. Allow **Microphone** when asked, and for sending the Mac's sound, turn on SoundPush under
   **System Settings → Privacy & Security → Screen & System Audio Recording**.
4. SoundPush lives in the menu bar.

### Linux (x64)

- Ubuntu/Debian: `sudo apt install ./SoundPush_<version>_amd64.deb`
- Fedora/openSUSE: `sudo dnf install ./SoundPush-<version>-1.x86_64.rpm`
- Any distribution: `chmod +x SoundPush_<version>_amd64.AppImage` and run it. The AppImage can update itself.

A desktop with a system tray (or the AppIndicator extension on GNOME) and PipeWire or PulseAudio is recommended.

### Android 8 or newer

1. On the phone, download `SoundPush_<version>_android….apk` and open it. Allow your browser or file manager to
   **install unknown apps** when Android asks.
2. Open SoundPush and allow **notifications** (they hold the stream controls). Microphone and screen-capture
   permissions are asked only when you first use a feature that needs them.

Preview APKs whose name ends in `debug-key-signed` are for testing. A later store or release-key build needs the
preview uninstalled first.

## 2. Pair your phone and computer

Both devices need the same Wi-Fi (or a hotspot or USB link, see [USB](#5-usb)). Pairing happens once per device.

**With the QR code (fastest)**

1. On the computer: Home → **Pair a device** (or Devices → **Pair**). A QR code appears; it is valid for 5 minutes.
2. On the phone: Home → **Scan pairing code** and point the camera at it.
3. Done. The phone appears under **Paired devices** on both sides.

**Without a camera (code pairing)**

1. On the phone: Devices → **Enter address**, and type the computer's address shown under
   Settings → About → *This computer* (for example `192.168.1.20`). Or pick the computer under **Nearby**.
2. Both screens show a 6-digit code. If the codes are **the same on both devices**, choose **Codes match** on each.
   If they differ, cancel: something on the network is interfering.

Paired devices reconnect automatically whenever both are on the same network. Under Devices you can rename a
device, turn off automatic connection, block it, or **Forget device**.

## 3. Everyday tasks

Start any task from either device. With one connected device it starts immediately; with several, SoundPush asks
which device to use. Active streams appear at the top of Home with the elapsed time, quality and delay, and a
**Stop** button. **Connection details** shows codec, buffer, jitter, loss and drift.

| On the computer | On the phone | What happens |
|---|---|---|
| **Listen on phone** | **Listen to computer** | The computer's sound plays on the phone |
| **Use phone as microphone** | **Use as computer microphone** | Apps on the computer hear the phone's microphone (set up the [virtual microphone](#4-virtual-microphone) first) |
| **Use phone as headset** | **Use as headset** | Both: listen and talk through the phone (headphones on the phone avoid echo) |
| **Hear phone audio** | **Send phone audio** | Sound from apps on the phone plays on the computer (Android 10+; Android asks to allow screen capture — only audio is captured) |

**Permissions.** When another device wants your microphone, SoundPush asks: **Allow** (this time),
**Always allow**, or **Don't allow**. Change it later under Devices → *device* → Permissions.

**While streaming from the computer:** mute the stream, change its volume, or **Mute PC speakers** so only the phone
plays. On the phone, the notification has Stop and Mute.

## 4. Virtual microphone

Other apps can only pick microphones the operating system knows, so "Use phone as microphone" feeds the phone's
voice into a virtual microphone device.

### Windows: VB-CABLE

1. SoundPush → **Audio** → **Install VB-CABLE**. SoundPush downloads the official, Microsoft-signed VB-CABLE package
   from VB-Audio (checked against a fixed checksum) and opens its setup: click **Install Driver**.
2. **Restart Windows** when asked (Audio → **Restart now**, or later yourself).
3. In Zoom, Meet, Teams, Discord or OBS choose **CABLE Output (VB-Audio Virtual Cable)** as the microphone.
   The Audio page always shows the exact name to pick.

VB-CABLE by VB-Audio is donationware; it stays installed if you uninstall SoundPush because other apps may use it.
A SoundPush-branded driver will replace it once it can be signed.

### macOS: SoundPush Microphone

1. SoundPush → **Audio** → **Install SoundPush Microphone** and enter your password. Nothing else is downloaded.
2. In your app, choose **SoundPush Microphone**. If it doesn't appear, click **Check again** or restart the Mac.
3. To remove it: Audio → **Remove SoundPush Microphone**.

### Linux

SoundPush is getting a built-in **SoundPush Microphone** PipeWire device that appears while the app runs, with no
setup. If your version's Audio page shows it as ready, choose **SoundPush Microphone** in your app. If the Audio page
says no virtual microphone is available, this feature is not in your version yet.

### Which device to feed

Audio → **Feed audio into** is **Automatic** by default and picks the installed virtual cable. SoundPush never uses a
real speaker as a microphone: that would play your voice out loud while apps hear nothing.

## 5. USB

USB is useful on busy or guest Wi-Fi, and gives the lowest delay.

1. Connect the phone to the computer with a USB cable.
2. On the phone: **Settings → Network & internet → Hotspot & tethering → USB tethering** (the name varies by brand).
3. The computer gets a network link to the phone; SoundPush finds and reconnects the paired device automatically.
   If it doesn't, pair or connect using the phone's address shown in SoundPush → Settings → About.

USB tethering may share the phone's mobile data with the computer; turn off mobile data on the phone if your plan is
limited. A USB mode without tethering (ADB) is planned.

A **phone hotspot** works the same way: connect the computer to the phone's hotspot.

## 6. Settings

**Audio (computer and phone)**

- **Latency:** *Low latency* (games, video; needs a strong link), *Balanced* (default), *Stable* (music, weak Wi-Fi,
  Bluetooth headphones), or *Custom* buffer limits.
- **Quality:** *Automatic* (adapts to the network), *Fixed bitrate*, or *Lossless* (USB or strong Wi-Fi).
- **Microphone:** input device, volume boost, noise suppression, *Listen to my microphone* (use headphones).
  On the phone also microphone mode, echo cancellation, device noise reduction and automatic gain.
- **Playback:** output, volume, balance, mono, audio delay for lip sync. On the phone: what happens when another app
  plays (pause, lower volume, keep playing) and pausing when headphones disconnect.

**General:** device name, theme (System/Light/Dark), language, start with the computer, start in the background,
keep running when the window is closed, keep the computer awake while streaming.

**Privacy & security:** who can find this device (*Everyone on the network*, *Paired devices only* — default,
*Nobody*) and automatic reconnection. On the phone, **Background** keeps SoundPush available to paired devices.

## 7. Keyboard and accessibility

Desktop:

- **Tab / Shift+Tab** move between controls; focus is always visible. **Space/Enter** activate.
- **Ctrl+1 … Ctrl+4** (⌘1 … ⌘4 on macOS) open Home, Devices, Audio and Settings.
- **Ctrl + plus / Ctrl + minus / Ctrl+0** (⌘ on macOS) change the text size.
- Choices such as Theme and Latency: **arrow keys**, **Home** and **End**.
- **Escape** closes dialogs that can be closed.
- Screen readers announce when a stream starts, reconnects or stops. Meters and progress bars have text values.
- SoundPush follows the system *reduce motion* setting and Windows high-contrast themes.

Android supports TalkBack, font scaling and the system dark theme. Code pairing works without a camera.

## 8. Updates

**Desktop.** With **Check for updates automatically** on (Settings → About; on by default), SoundPush looks for a new
version once a day. When one is available, a banner appears at the top of the window:

1. **Download update** — the banner shows the progress. Every update is verified against SoundPush's signing key
   before it can be installed.
2. **Restart to install** — SoundPush restarts on the new version. On Windows the installer runs briefly.

Choose **Later** to hide the banner; the update stays available in Settings → About. **Check for updates** checks
immediately and tells you if you're up to date or if the check failed (for example when offline). Automatic checks
never show errors. If installing fails, download the new version from the releases page.

**Update channel** (Settings → About) chooses which versions you get. **Stable** (the default) gets tested releases;
they reach everyone within about three days, so an update can appear for someone else before it appears for you, and
**Check for updates** gets it right away. **Beta** gets new versions as soon as they are published, before stable;
they may have more bugs. Switching back to Stable keeps the version you have until a newer stable release arrives.
Installs from Flathub are updated by Flatpak instead of the app.

**Android.** Settings → About shows when a newer version is on GitHub, with a link to download it. Turn off
**Check for updates automatically** there if you install updates another way (for example from F-Droid or Google Play).

## 9. Troubleshooting

<!-- --8<-- [start:troubleshooting] -->
The desktop app also has short tips under **Settings → Help & diagnostics**.

### The devices can't find each other

- Both must be on the **same network**. Guest networks, public hotspots and some office/university Wi-Fi block
  devices from seeing each other ("client isolation"). Use the phone's **hotspot** or **USB tethering** instead.
- Windows: SoundPush shows a **Fix** button when the firewall is in the way. It adds one rule for SoundPush only,
  and — if you tell it the network is your own — moves that network to the **Private** profile, in a single
  administrator prompt. A rule for private networks does nothing while Windows still calls the network public.
- VPNs on either device can hide the local network; pause them.
- Try connecting by address (Devices → Enter address) using the address in Settings → About.
- Check **Who can find this computer** isn't set to *Nobody* while pairing.

### No sound

- Check the volume and mute state on both devices, and the **Output** under Audio.
- Sending the computer's sound: make sure something is playing on the output selected under **Audio → Audio to send**
  (*System default* follows Windows/macOS).
- macOS: turn on SoundPush in **Privacy & Security → Screen & System Audio Recording**; without it macOS sends silence.
- Android: another app may have taken audio focus; check **When another app plays** in the phone's Audio settings.

### The microphone doesn't show up in Zoom/Discord/Meet

- Set up the [virtual microphone](user-guide.md#4-virtual-microphone) and restart Windows after installing VB-CABLE.
- In the app, select **CABLE Output** (Windows) or **SoundPush Microphone** (macOS) — restart the app after installing.
- Start **Use phone as microphone** and allow the microphone on the phone.

### Audio crackles, cuts out or lags

- Choose **Stable** latency, move closer to the router, prefer 5 GHz Wi-Fi, or use USB.
- Bluetooth headphones on the phone add delay; use *Stable* or adjust **Audio delay (lip sync)**.
- Close apps that download heavily on the same network.

### It keeps disconnecting on the phone

- Settings → Background → **Keep SoundPush running**: allow SoundPush to run in the background and turn off battery
  optimisation for it. On Xiaomi/Redmi/POCO also allow **Autostart** and set battery saver to *No restrictions*.
- Keep the notification; swiping SoundPush away from recent apps can stop streams on some phones.

### macOS keeps asking for the microphone

Install the latest build (it is signed consistently), then reset the permission once:
`tccutil reset Microphone net.soundpush.desktop`.

### An update won't install

Download the latest version from the [releases page](https://github.com/FlyToRakib/soundpush/releases/latest) and
install it over the current one; your paired devices and settings are kept.

### Reporting a problem

1. Settings → Help & diagnostics → **Export diagnostics**. The report is saved on your computer; it contains no audio,
   and remote addresses are removed. Read it before sharing.
2. Settings → Help & diagnostics → **Report a problem** opens GitHub. Describe what you did and attach the report.
   On Android, phone logs can be collected with `adb logcat -s SoundPush`.

Security problems: please report privately (see [SECURITY.md](https://github.com/FlyToRakib/soundpush/blob/HEAD/SECURITY.md)).
<!-- --8<-- [end:troubleshooting] -->

## 10. Uninstall and your data

- **Windows:** Settings → Apps → SoundPush → Uninstall. Tick **Delete the application data** to also remove paired
  devices, settings and logs. VB-CABLE stays installed (remove it from Apps if you don't need it).
- **macOS:** remove the SoundPush Microphone (Audio page) first, then move SoundPush to the Bin.
  Data: `~/Library/Application Support/SoundPush`.
- **Linux:** `sudo apt remove soundpush` / `sudo dnf remove soundpush`, or delete the AppImage.
  Data: `~/.local/share/SoundPush`.
- **Android:** uninstall the app; its data is removed with it.

What SoundPush stores and why: [privacy policy](privacy.md).
