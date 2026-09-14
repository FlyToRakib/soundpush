# Privacy policy

SoundPush is free, open-source software for routing audio between your own devices. It has no accounts, no ads,
no analytics and no telemetry. **Audio never leaves your devices** except to the devices you pair.

This policy covers the SoundPush desktop app (Windows, macOS, Linux) and the SoundPush Android app.
Last updated: 2026-09-14.

## What SoundPush does not do

- It does not collect, sell or share personal data. The SoundPush project runs no servers that receive your data.
- It does not include analytics, advertising, crash-reporting or tracking SDKs.
- It does not upload crash reports, logs or diagnostics. Nothing is sent unless you choose to share it yourself.
- It does not record audio. Audio is streamed live between paired devices and is not stored.

## Data that stays on your devices

| Data | Why | Where |
|---|---|---|
| Device identity key | Proves this device's identity during pairing and every connection | Desktop: OS keychain (Windows Credential Manager, macOS Keychain, Linux Secret Service) plus an encrypted file in the SoundPush data folder. Android: app-private storage, wrapped by Android Keystore |
| Paired devices (trust store) | Names, public keys, permissions and last known addresses of devices you paired | Encrypted file in the SoundPush data folder |
| Settings | Your choices (device name, theme, audio options) | `settings.json` in the SoundPush data folder |
| Logs | Help you and us understand problems. They never contain audio, keys, pairing secrets or full device fingerprints | `logs` folder, rotated automatically |
| Diagnostics reports | Created only when you click **Export diagnostics** | `diagnostics` folder |

SoundPush data folder: Windows `%LOCALAPPDATA%\SoundPush`, macOS `~/Library/Application Support/SoundPush`,
Linux `~/.local/share/SoundPush`, Android app-private storage.

On Android, the identity key and the list of paired devices are excluded from cloud backups and device-to-device
transfers, so a restored phone never clones another device's identity.

## Network connections

SoundPush connects to other devices and services only for these purposes:

- **Your paired devices, on your local network or USB link.** All audio and control messages are end-to-end
  encrypted (QUIC with TLS 1.3) and authenticated with the keys exchanged during pairing.
- **Discovery on your local network.** SoundPush announces itself with mDNS and a small signed broadcast so your
  devices can find each other. Under Settings → Privacy you choose who can see this device; with "Paired devices
  only" (the default) the device name is not broadcast.
- **Update checks (GitHub).** When "Check for updates automatically" is on (the default), the desktop app downloads a
  small version file from GitHub Releases once a day, and the Android app asks the GitHub API for the latest release
  when you open Settings. GitHub receives your IP address and a standard request header, as with any website visit;
  see [GitHub's privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement).
  Turn it off in Settings → About. Updates are never installed without your action.
- **Virtual microphone setup (Windows).** When you choose **Install VB-CABLE** on the Audio page, SoundPush downloads
  the official VB-CABLE package from vb-audio.com.
- **Links you open.** "User guide", "Report a problem" and similar links open in your web browser.

## Sharing diagnostics

**Export diagnostics** writes a text report to your computer: app version, operating system, the app's current
state with remote addresses removed and device IDs shortened, and recent log lines. Read it before you share it.
You decide whether to attach it to a GitHub issue; SoundPush never sends it anywhere.

## Permissions

- **Microphone:** only while a stream that uses the microphone is running.
- **Camera (Android):** only to scan a pairing QR code. Images are not stored.
- **Screen capture / system audio recording:** only to send app or system audio you asked to send. On Android this
  captures audio only, never the screen image.
- **Notifications (Android):** to show stream controls while streaming.

Other devices can use your microphone only if you allow it; the default for the microphone is to ask each time.

## Children

SoundPush does not knowingly collect data from anyone, including children.

## Changes and contact

Changes to this policy are published in this file with the date above and noted in `CHANGELOG.md`.
Questions: open a discussion at <https://github.com/FlyToRakib/soundpush/discussions>. Security issues: see
[`SECURITY.md`](SECURITY.md).
