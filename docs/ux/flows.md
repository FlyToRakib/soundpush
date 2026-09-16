# User flows

What the two apps actually do, screen by screen. This describes the code as it stands, so it is a
reference for reviewing a change and for writing tests — not a wish list. The plan's intent is in
`docs/soundpush-final.md` §23; where the two disagree, the code is what ships and this file follows it.

Desktop means the Tauri + Svelte app in `sound-push-desktop/ui/src`. Phone means the Kotlin + Compose
app in `sound-push-mobile/android`.

## Information architecture

**Desktop** — a fixed sidebar with four items (`App.svelte`): Home, Devices, Audio, Settings. There is
no router and no URL; the current page is a value in `lib/stores/ui.svelte.ts`. Ctrl/⌘ + 1…4 switch
pages and move focus into `<main>`, which is labelled after the page it shows. Three whole-app states
exist: the shell, a `role="alert"` panel when the engine could not start (with "Open logs folder"),
and a `role="status"` spinner while it is starting. A language change re-renders the whole tree and
sets `lang` and `dir` on the document.

**Tray** (`src-tauri/src/tray.rs`) — a status line, one submenu per running route with Mute and Stop,
"Mute microphone" with its hotkey, "Stop all streams", a "Recent devices" submenu of at most five, and
Open / Quit. The menu is a data description diffed against the last one, so per-second statistics never
rebuild it. The icon carries a badge: error, then attention, then microphone live, then streaming.
On Linux the tray is only used when something owns `org.kde.StatusNotifierWatcher`.

**Phone** — three tabs (`AppChrome.kt`): Home, Devices, Settings. Audio, the troubleshooter, the battery
guide, the security log and the licences are Settings sub-screens and keep the Settings tab selected
while showing a back arrow. A window wider than compact swaps the bottom bar for a side rail, and Home
and Devices become two-pane. The scanner and onboarding are full screen with no chrome. Outside the app
there is a Quick Settings tile (toggle listening) and a home-screen widget (listen, stop, mute).

## First run

**Desktop** (`features/Onboarding.svelte`) — a dialog with three steps, skippable at every one:

1. Welcome.
2. Name this computer, pre-filled, plus a "Start with Windows" / "Open at login" toggle.
3. Pair: a pairing QR, alongside a smaller QR that points at the Android app download.

Pairing a phone ends onboarding by itself. Finishing writes the name, the autostart choice and the
`onboarding` tip, and stops pairing.

**Phone** (`app/Onboarding.kt`) — the steps are decided once, so granting a permission mid-flow does not
renumber them: welcome, notifications (only below a granted `POST_NOTIFICATIONS` on Android 13+), the
OEM battery guide (only when the phone is a make with known restrictions), and pair. The pair step lists
nearby computers so a phone without a working camera can pair with one tap, and offers "Enter address"
and a share sheet for the desktop download. Pairing finishes the flow.

The phone never asks for a device name during onboarding (it is in Settings) and never asks for the
microphone or camera until the feature is first used — the welcome step says so.

## Pairing

1. One side shows a code: desktop Home → "Pair a device", or the phone's onboarding. The QR carries a
   one-time secret and expires; the desktop dialog counts down and asks for a new code once, at most
   every five seconds.
2. The other side scans it (phone: Devices → "Scan code") or connects by address (both sides offer
   "Enter address", for a computer found by IP rather than by discovery).
3. **Both** devices then show the same six-digit code, split as `123 456`, and both must confirm. The
   dialog cannot be dismissed. The desktop reads the code out digit by digit for a screen reader and
   says when the other side has already confirmed.
4. Success is an engine notice — "Paired with *name*" — shown as a toast or a snackbar. The pairing
   dialogs close themselves when the trusted-device list grows.

Failures are engine errors with their own text: an expired code, a cancelled confirmation, or too many
attempts (which pauses pairing from that source for a minute). Every attempt, accepted or not, is
written to the security log.

Pairing from a nearby list on the phone adds a hint: "Now open “Pair a device” on *name* and confirm
the code." — because that side has to be looking.

## Starting a stream

Both apps show the same four task cards, each from the local device's point of view:

| Desktop | Phone | Route kinds |
|---|---|---|
| Listen on phone | Listen to computer | `sendSystemAudio` / `receiveSystemAudio` |
| Use phone as microphone | Use as computer microphone | `receiveMicToVirtualMic` / `sendMicToVirtualMic` |
| Use phone as headset | Use as headset | both of the above |
| Hear phone audio | Send phone audio | `receiveAppAudio` / `sendAppAudio` |

A card that cannot run says why, in its own description, and stays tappable so it can repeat the reason:
nothing connected, no connected device can do this, the computer has no virtual microphone, or Android
is too old for app audio. The headset task claims its two parts, so starting it lights one card rather
than three.

With exactly one connected device a tap starts the stream. With more than one, both apps always ask —
"so audio never goes to a device the user didn't pick". In the picker, a device that cannot do the task
is shown but not selectable; the phone gives the specific reason per device, the desktop a generic one.
The picker closes by itself if every device disconnects.

On the receiving device, unless its permission for that kind is already "Allow", a prompt appears naming
the asker and what it wants. The desktop offers Don't allow / Always allow / Allow; the phone offers
Don't allow / Allow with a "Remember for this device" checkbox. When the phone app is not in front, the
request arrives as a high-priority notification instead.

Route cards then show the stream: what it is, how long it has been running, a quality badge with the
measured latency, Mute, Stop, and — expanded — volume, "Mute computer speakers", and the connection
details (codec, bitrate, buffer, jitter, loss, drift, addresses, transport).

## Using the phone as a microphone

The computer needs a virtual microphone — an input device other apps can choose. Audio → Virtual
microphone reports one of four states and always offers one action:

- **Ready**: it names the device to pick in Meet, Zoom, Discord or a recorder. On Windows, where the
  bundled cable is VB-CABLE, the VB-Audio credit and link are shown as their licence requires.
- **Supported, not installed**: an Install button. macOS asks for a password; Windows downloads the
  official VB-CABLE package, opens its setup and then offers "Restart now" with a dialog that says
  exactly what will happen. Closing the password prompt is treated as a choice, not an error.
- **A speaker was chosen by mistake**: a banner explains that it would play your voice out loud and
  other apps would hear nothing, with one action to go back to automatic.
- **Unsupported platform**: a link to the vendor and a "Check again" button.

"Start phone microphone when an app uses it" makes the computer ask the phone as soon as another app
opens the virtual microphone; when that fires, a notice says so.

On the phone, starting a microphone route checks the runtime permission first and remembers the answer,
so a first refusal explains what stopped working while a permanent one offers app settings. Streams work
without notifications, so a missing notification permission is explained alongside the start rather than
blocking it. While the route runs, the foreground service takes the microphone type and the notification
says "Microphone in use" with a Mute action. If another app takes the microphone — a call, an assistant —
Android hands SoundPush silence: the notification and a Home banner say so, and both clear themselves
when the other app lets go.

## Headset

Both sides start the two routes in sequence with the same device, and the phone asks for the microphone
permission once for the pair. While the headset task runs, the phone's recorder switches to the voice
preset with echo cancellation so the phone's own output is not sent back; the stored microphone settings
are untouched and the recorder returns to them the moment the task ends.

## Sending the phone's app audio

Android asks for screen-capture consent every time, and it cannot be remembered. The service takes the
`mediaProjection` foreground type *before* the projection is created, as Android 14 requires. Refusing
leads to a dialog that explains the every-time rule and offers to try again.

A route that opened without the app on screen — a remembered permission, or resume-on-start — has no
recorder and would carry silence. If the app is visible, it asks for consent after a moment; if consent
is refused, those routes are stopped rather than left silent. If the app is not visible, a notification
asks the user to open it. When the user taps "Stop sharing" in the status bar, the routes end.

## Stopping and muting

Every surface controls the same state, so none of them can disagree:

| | Stop | Mute |
|---|---|---|
| Desktop app | per route | per route |
| Tray | per route, plus "Stop all streams" | per route, plus the microphone |
| Phone app | per route | per route |
| Notification | stops every route | playback and microphone separately |
| Media controls | Stop | Play / Pause |
| Quick Settings tile | stops listening | — |
| Widget | stops listening | mute listening |

The notification and the widget read the routes' own mute flags rather than a local copy, so a mute set
from the computer is reflected there too. Audio focus, a phone call in "Keep playing" mode, and
unplugging headphones all mute receiving routes automatically, and the focus handler unmutes only what
it muted.

## Devices

Selecting a paired device opens its detail (a pane on wide windows, a bottom sheet on a phone):

- Rename it locally, and turn automatic reconnection on or off.
- Four permissions — receive my audio, use my microphone, send audio to me, control me — each Allow /
  Ask / Don't allow.
- A per-device audio profile: latency, quality, bitrate, extra protection. Each falls back to the
  global setting; the desktop shows what that currently is.
- A connection test that measures delay, jitter, loss and speed for about ten seconds, reports tips,
  and offers to apply the settings it recommends.
- Block, and Forget with a confirmation that says the device will have to pair again.

## Help

The desktop troubleshooter is an accordion inside Settings with five topics; the phone's is a list of
four, each opening its own screen. Both run their checks before asking the user anything, sort problems
first and tips last, give each check one action that fixes it, and re-run when the user comes back from
a system settings screen. Fixes are real: open the firewall dialog, switch to the Stable profile, unmute
the microphone, clear a wrongly chosen virtual microphone, open the right system settings page.

Diagnostics export is a text file, never an upload. The desktop previews it first — sections, line
counts and exactly what was removed (addresses, shortened device IDs, the pairing code) — with a
disclosure that shows the full contents in a focusable, selectable text box. The phone writes and shares
the same content through the share sheet, and deletes exports left in the cache when memory is short.

A crash on the previous run is mentioned once, and the report stays on the device until the user
exports it.

## Settings that take effect immediately

- **Theme** — desktop applies it to the document and the native title bar and mirrors it to
  `localStorage`, so the next window opens in the right theme before the engine answers; the phone
  re-derives it and matches the status and navigation bar icons to the app theme, not the system one.
- **Language** — desktop re-renders the whole tree and sets `lang`/`dir`; the phone writes the engine
  setting first and then hands the locale to Android, which recreates the screen. Both mirror the value
  back if it changes elsewhere. Pseudo-locales (long text, right to left) appear in development builds.
- **Latency and quality** — identical option sets and bounds on both platforms, applied to running
  routes without a restart. A per-device profile overrides the global one; the engine can override both
  on an unstable link, and says so in a notice.
- **Output device (phone)** — the list is the outputs connected right now, re-read as they come and go;
  the notification's Output action opens the system switcher through a four-step fallback chain.
- **Stay available (phone)** — turning it on also asks for the battery exemption, because that is exactly
  when it is justified.
- **Resume streams after restart** — the engine restores the streams that were running once the device
  is back. It is off by default on both platforms.

## Background behaviour

The desktop keeps running when its window closes, and says so once with a dialog that offers "Quit" and
"Keep running" (or "Minimize" where there is no tray).

The phone keeps a foreground service while a stream runs or "Stay available" is on, with the service
types that match what is active. Wi-Fi and wake locks are held only while audio is flowing. While the
app is in the background or the screen is off, statistics, meters and elapsed time stop being decoded
and drawn; anything anyone can act on still arrives, and the stream itself is untouched.
