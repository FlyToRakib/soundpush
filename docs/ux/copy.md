# UX copy

The wording rules both apps already follow, written down so new strings match. Every rule below is
observable in `sound-push-desktop/ui/src/lib/i18n/en.json` and
`sound-push-mobile/android/core-ui/src/main/res/values/strings.xml`; the examples are real strings.

English is the source language for both apps. Translations are managed in Weblate — see
[translating](../translating.md).

## Voice

**Plain, second person, no apology.** Say what happened and what to do. Address the reader as "you" and
name the devices as "this computer", "this phone", or by their own name.

> "Couldn't reach the device. Check that it's on and on the same network."

**Contractions, always.** `couldn't`, `can't`, `isn't`, `doesn't`, `won't`, `you'll`. "Cannot" and
"do not" read like a warranty.

**Never the implementation's words.** There is no "server", "client", "host", "peer", "socket" or
"backend" anywhere in the UI, and there never should be. The nouns are **device**, **stream**,
**virtual microphone**, **pairing code**, **sound**. "Hostname" appears once, in "IP address or
hostname", because that is what the user is typing. Protocol names (QUIC, TLS over TCP, Opus) appear
only in connection details and diagnostics, never on a task or a button.

**No blame and no filler.** Nothing is the user's fault, nothing is "unfortunately", and a choice is
not an error: declining a Windows UAC prompt or a macOS password prompt is treated as "not now", not as
a failure. "Please" appears once in both apps combined.

**Explain the consequence, not the rule.** When a setting matters, say what will happen:

> "“*Name*” is a speaker, not a virtual cable, so SoundPush doesn't use it. It would play your voice
> out loud and other apps couldn't hear it."

## Sentence case

Everything is sentence case: headings, buttons, menu items, settings labels, tray entries. Capitalise
only the first word and proper nouns.

- "Pair a device", "Stop all streams", "Security log", "Who can find this computer"
- Product and platform names keep their own casing: SoundPush, VB-CABLE, VB-Audio, BlackHole,
  PipeWire, PulseAudio, Windows, macOS, Android, Google Meet, Zoom, Discord, OBS.
- Mode names are labels and keep their capital: "Low latency", "Balanced", "Stable", "Excellent".
- A setting quoted from another app or from the OS is copied exactly, in curly quotes: "Never sleeping
  apps", "Install Driver", "No restrictions".

## Errors

An error says **what happened**, then **what to do**, in two short sentences:

| | |
|---|---|
| ✅ | "The pairing code expired. Show a new code." |
| ✅ | "This Wi-Fi blocks devices from talking to each other. Try USB or your hotspot." |
| ✅ | "This device was removed. Pair it again to continue." |

When there is nothing useful to add, the error is one clause and the *button* carries the action —
"Device not found", "The audio device isn't available." The last resort is "Something went wrong."

Error codes never reach the user. Engine error keys (`error.network.unreachable`) are stable
identifiers for the code and the docs; the desktop looks the key up directly in `en.json`, and Android
maps it through an explicit table in `core-ui/.../Labels.kt` — by hand, because looking resources up by
name at runtime breaks under resource shrinking. A key with no text falls back to the generic message,
so an engine that learns a new error never shows a raw key.

The same namespacing applies to engine notices (`notice.pairingSuccess`), suggested fixes
(`fix.switchToUsb`, which becomes the banner's one button) and security-log entries (`audit.*`).

## Problem banners

One problem, one primary fix, optionally "Learn more". A banner that describes a live condition — the
microphone taken by another app, notifications off while a stream runs — is not dismissible, because it
disappears by itself when the condition ends. A tip is dismissible and stays dismissed.

## Empty states

One sentence and one action.

> "Pair your phone to get started" — "Scan a code with the SoundPush app on your phone. It takes a few
> seconds." — **Pair a device**

An inline empty line inside a card is a single complete sentence with a full stop: "No paired devices
yet.", "No other SoundPush devices found on this network.", "Nothing recorded yet." A measurement that
does not exist yet is an em dash, not "N/A" or "0".

## Status

Status language is what the user can see, not what the socket is doing:

> "Streaming with *Pixel 8* · 12:04 · Good · 42 ms"

The status vocabulary is shared by both apps, word for word: Connected, Weak connection, Connecting…,
Reconnecting…, Waiting for device, Offline, Pairing…, Update needed. Link quality is Excellent / Good /
Poor, always with an icon and text, never colour alone. IP addresses, fingerprints and raw statistics
live behind "Connection details".

## Punctuation and numbers

- **Ellipsis**: the character `…`, never three dots, and only for something in progress: "Starting…",
  "Reconnecting…", "Installing…", "Testing… 40%".
- **Quotes**: curly double quotes `“ ”` around a name the user will see elsewhere — a device, an app, a
  setting. Apostrophes are straight (`'`), consistently, in both files.
- **Dashes**: an en dash for ranges — "150–250 ms", "Android 8–12". A middle dot `·` joins parts of one
  status line. An arrow `→` shows direction ("Microphone → *Laptop*") and settings paths
  ("Settings → About").
- **Units**: a space between number and unit — `42 ms`, `128 kb/s`, `+6 dB`, `5 GHz`, `3 × 2 MB`.
  `kb/s` is lower case.
- **Terminal punctuation**: buttons, labels and captions on a card have none; body copy, descriptions
  and errors are full sentences and end with a full stop.
- **Wi-Fi**, **Bluetooth**, **USB** are spelled exactly like that. `adb` stays lower case.
- **Interpolation is positional** — `{0}` on the desktop, `%1$s` on Android — so a translation can
  reorder. A device name goes in bare; an app, device or setting name the user must recognise goes in
  curly quotes. Units are separate strings on Android for the same reason.

## Permissions and consent

Say what the permission is for before asking for it, and say what stops working without it.

> "SoundPush uses the camera only to scan pairing codes. Open app settings, tap Permissions and allow
> Camera, or pair by entering the computer's address instead."

A permission choice is "Allow" / "Ask" / "Don't allow" — never "Deny" or "Block". A request names who is
asking and what they want: "*Laptop* wants to use your microphone". Something the user cannot be given
is explained, not hidden: screen capture must be granted every time, and the dialog says so.

## Privacy claims

State them plainly and without hedging, where the user is making the decision:

> "Audio never leaves your devices. SoundPush has no accounts, ads or tracking."

Say what is kept and for how long where it is kept: "Pairing, permission and stream events on this
computer. Kept for 30 days and never sent anywhere."

## Accessibility

Copy is part of the accessibility work, not a separate pass.

- **Icon-only controls carry the action, not the state.** A Mute button says "Mute" and becomes
  "Unmute"; the icon shows which it is.
- **Status is spoken before the text.** A troubleshooter row announces "Needs attention" / "OK" / "Tip"
  and then reads the check; the coloured dot is hidden from screen readers.
- **Route changes are announced** through a live region — "*Listening to Laptop*: streaming started" —
  and the routes that were already running when the window opened are not announced, because they are
  not news.
- **A code is read as digits.** The pairing code sets a per-digit label so "482913" is not read as a
  number.
- **Charts and meters have words.** The latency breakdown is an image with a text label listing each
  stage and its milliseconds; the chart has a one-sentence summary; the level meter reads "−18 dB".
- **Rows read as one item.** A label, its value and its state are merged into a single node so a screen
  reader says "Jitter, 1.2 ms" rather than two fragments.

## Differences between the two apps

Most strings are shared word for word — the whole security log, the status vocabulary, the latency and
quality names, the permission policies, the network test. Where they differ, it is because the sentence
is about a different device: the desktop says "Listen on phone" and the phone says "Listen to computer";
the desktop says "Name on this computer" and the phone "Name on this phone".

Two things are worth knowing when adding a string:

- The **tray menu is written in Rust** (`src-tauri/src/tray.rs`) and is not translated. A new tray entry
  duplicates wording that exists in `en.json`; keep the two identical.
- Android has **no plurals**; the desktop uses `_one` / `_other` with `Intl.PluralRules` for the one
  place it counts devices. Prefer a phrasing that does not need a plural.

## Checklist for a new string

1. Sentence case, no terminal full stop on a button or label.
2. Second person, contractions, no implementation words.
3. If it is an error: what happened, then what to do.
4. If it names a device the user must recognise elsewhere: curly quotes.
5. Positional placeholders, units as their own string on Android.
6. Added to `en.json` **or** `strings.xml` — and to both, with identical wording, when the same thing
   exists on both platforms.
7. Anything read aloud: check the content description says the action, and that the row merges.
