# Testing SoundPush (phone ↔ computer)

A hands-on checklist for testing every feature with a real phone and computer.

## Before you start

- Phone and computer on the **same Wi-Fi** (not a guest network).
- Desktop app running (menu bar / tray shows SoundPush).
- Phone app installed. For speed, use the **release** build:
  `sound-push-mobile/android/app/build/outputs/apk/release/app-release.apk`
  (the debug APK starts much more slowly).
- Keep the phone app **open on screen** while testing. Some phones (e.g. Xiaomi/MIUI) freeze
  apps in the background; SoundPush keeps running only while a stream is active.
  On MIUI also set: Settings → Apps → SoundPush → Battery saver → **No restrictions**, and enable **Autostart**.

## 1. Pairing

1. Computer: Home → **Pair a device** (a QR code appears).
2. Phone: **Scan pairing code** → point at the QR code.
3. ✅ Both apps list the other device as **Connected**.

No camera? Phone: Devices → **Enter address** → type the computer's IP (Computer: Settings → About).
Both screens then show a 6-digit code; tap **Codes match** on both.

## 2. Phone sound → computer speakers

1. Phone: Home → **Send phone audio** (Android 10+).
2. Allow the **screen/audio capture** prompt (Android requires it every time).
3. Play music or a YouTube video on the phone.
4. ✅ The sound comes out of the computer. The route card shows elapsed time and latency.

Apps that block capture (some banking/DRM video apps) stay silent. That's an Android rule, not a bug.

## 3. Computer sound → phone

1. Computer: Home → **Listen on phone** (or phone: Home → **Listen to computer**).
2. **Mac (first time):** macOS asks to allow **recording system audio**. Click Allow.
   If you dismissed it: System Settings → Privacy & Security → Screen & System Audio Recording → enable SoundPush (or the terminal/app that launched it during development).
3. Play something on the computer.
4. ✅ The sound plays on the phone.

Extras to try on the route card:
- **Mute computer speakers** (phone card → expand): the phone keeps playing.
- Volume slider, **Mono**, **Audio delay (lip sync)** under Audio settings.
- Latency: Settings → Audio → Low latency / Balanced / Stable.

## 4. Phone microphone → computer

**As speaker output (quick check):** the computer asks for the phone mic → the phone shows
"… wants to use this phone's microphone" → **Allow**. Speak; ✅ you hear it on the computer.

**As a microphone for Zoom/Discord (virtual mic):**
1. Install a free virtual cable on the computer (Mac: BlackHole 2ch; Windows: VB-CABLE).
2. Computer: Audio → Virtual microphone → **Feed audio into** → BlackHole 2ch / CABLE Input.
3. Computer: Home → **Use phone as microphone**.
4. In Zoom/Discord choose **BlackHole 2ch** / **CABLE Output** as the microphone.
5. ✅ The app's mic level moves when you talk into the phone.

Try: Settings → Audio → Microphone mode, Volume boost, Noise suppression, echo cancellation.

## 5. Headset mode

Computer: Home → **Use phone as headset** (needs the virtual mic from step 4).
✅ Computer sound plays on the phone **and** the phone mic reaches the computer.

## 6. Reliability checks

| Action | Expected |
|---|---|
| Turn phone Wi-Fi off for 5 s, then on | Card shows "Reconnecting…", audio resumes by itself |
| Quit and reopen the desktop app | Phone reconnects automatically |
| Unplug headphones from the phone while listening | Playback mutes (if "Pause when headphones disconnect" is on) |
| Phone call during playback | Playback pauses and resumes after the call |
| Devices → Forget device | The device can no longer connect until paired again |
| Permissions → "Can use this phone's microphone" = Don't allow | Mic requests are refused |

## 7. Automated checks

Everything here runs without a phone. `just lint`, `just core-test`, `just desktop-check` and
`just mobile-check` are what CI runs on every pull request.

| Command | What it covers |
|---|---|
| `just mobile-check` | The Android gate: debug APK, App Bundle, unit and screenshot tests, Android lint, ktlint |
| `just mobile-format` | Fixes the Kotlin style violations ktlint can fix by itself |
| `just desktop-check` | ESLint, Prettier, svelte-check and the production build |
| `just desktop-format` | Rewrites the formatting differences Prettier reports |
| `just latency --link congested` | End-to-end pipeline latency over a simulated link |
| `just netsim list` | The impairment profiles, and how to apply them to a real interface |
| `cargo test -p sp-audio-io --test wasapi_smoke -- --nocapture` | WASAPI enumeration, render and loopback (Windows) |

### Startup and jank (Macrobenchmark)

Measure against a **real phone**, which is the only number worth quoting. Connect it with USB
debugging on and run:

```sh
just mobile-benchmark
```

Results land in `sound-push-mobile/android/benchmark/build/outputs/connected_android_test_additional_output`.
The nightly *Android benchmark* workflow runs the same thing on an emulator and can be started by hand
from the Actions tab; an emulator's numbers are a trend, not a verdict, which is why the pull-request
gate leaves them out.

### Dependency verification

Gradle checks every downloaded dependency against
`sound-push-mobile/android/gradle/verification-metadata.xml` (plan §31), so the Android build fails if
an artifact changes underneath it. After changing a dependency or a plugin version, regenerate the
file with `just mobile-verification` and read the note in that recipe about the `aapt2` entries for
the operating systems your machine is not.

## If something fails

- Computer: Settings → Help & diagnostics → **Export diagnostics**, and send the file.
- Phone logs (USB debugging on): `adb logcat -s SoundPush`
