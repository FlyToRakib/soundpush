# Error codes

Every error SoundPush shows carries a short code such as `SP-NET-004`. The code is the same in
every language and on every platform, so you can quote it in a bug report or a discussion and we
know exactly which failure you hit.

Where to find a code:

- in the message itself, after the text;
- in a **diagnostics export** (Settings → Help), next to the message;
- in the **log files** (Settings → Open logs), as `code=SP-NET-004`.

Codes never change and are never reused. A code you do not find below comes from a newer version:
check that this page matches your app version (Settings → About).

## Connection — `SP-NET`

| Code | Meaning | What to do |
|---|---|---|
| `SP-NET-001` | Could not reach the device. | Check that the other device is awake, running SoundPush and on the same network. See [troubleshooting](troubleshooting.md). |
| `SP-NET-002` | The device is not available any more. | Refresh the device list, or connect the device again. |
| `SP-NET-003` | A firewall is blocking incoming connections. | Use the "Allow SoundPush" fix on the message, or allow SoundPush in your firewall for private networks. |
| `SP-NET-004` | The network blocks devices from talking to each other (client isolation, common on guest Wi-Fi). | Use a different network, your phone's hotspot, or a USB connection. |

## Pairing and trust — `SP-SEC`

| Code | Meaning | What to do |
|---|---|---|
| `SP-SEC-001` | The device is not paired. | Pair the two devices first. |
| `SP-SEC-002` | Pairing was declined or the codes did not match. | Start pairing again and compare the codes carefully. |
| `SP-SEC-003` | The pairing code expired. | Show a new code and scan it again. |
| `SP-SEC-004` | Too many pairing attempts from the same address. | Wait a minute and try again. |
| `SP-SEC-005` | The device was removed or blocked. | Pair it again, or unblock it under Devices. |

## Versions — `SP-CMP`

| Code | Meaning | What to do |
|---|---|---|
| `SP-CMP-001` | The other device runs a SoundPush version this one cannot talk to. | Update SoundPush on both devices. |

## Permissions — `SP-PRM`

| Code | Meaning | What to do |
|---|---|---|
| `SP-PRM-001` | The other device did not allow this. | Ask its user to allow the stream, or change the device's permissions there. |
| `SP-PRM-002` | Microphone access is switched off for SoundPush. | Allow the microphone in the system settings. |

## Audio — `SP-AUD`

| Code | Meaning | What to do |
|---|---|---|
| `SP-AUD-001` | An audio device could not be opened or was unplugged. | Choose another device, or reconnect the one you want. |
| `SP-AUD-002` | This device cannot capture its own system audio. | On Windows 10 N, install the Media Feature Pack; on macOS, allow System Audio Recording. |
| `SP-AUD-003` | The virtual microphone is not installed. | Set it up on the [virtual microphone](virtual-microphone.md) page. |
| `SP-AUD-004` | Another device is already the virtual microphone. | Answer "Replace" when SoundPush asks, or stop the other device's route first. |

## Settings and input — `SP-CFG`

| Code | Meaning | What to do |
|---|---|---|
| `SP-CFG-001` | The stream is no longer there. | It already stopped; nothing to do. |
| `SP-CFG-002` | A value was not valid (an address, a name, a pairing code). | Check what you entered. |
| `SP-CFG-003` | The maximum number of devices listening to this source is reached. | Stop one of the streams, or raise the limit in Settings → Advanced. |

## Application — `SP-SYS`

| Code | Meaning | What to do |
|---|---|---|
| `SP-SYS-001` | Settings or paired devices could not be saved. | Check that the disk is not full and that SoundPush may write to its data folder. |
| `SP-SYS-002` | SoundPush is shutting down. | Start it again. |
| `SP-SYS-003` | SoundPush is still starting. | Wait a moment and try again. |
| `SP-SYS-004` | The action was cancelled. | Nothing to do. |
| `SP-SYS-005` | Something unexpected went wrong. | Please [report it](https://github.com/FlyToRakib/soundpush/issues/new/choose) with a diagnostics export. |

## Why a stream stopped — `SP-SES`

These appear in logs and diagnostics when a stream ends, on either device. The number is the
reason's value on the wire, so it is stable across versions.

| Code | Reason |
|---|---|
| `SP-SES-000` | No reason given. |
| `SP-SES-001` | You stopped it. |
| `SP-SES-002` | The other device stopped it. |
| `SP-SES-003` | Permission denied. |
| `SP-SES-004` | Permission was withdrawn while streaming. |
| `SP-SES-005` | The device was busy (for example, its limit of listeners was reached). |
| `SP-SES-006` | The audio device was lost. |
| `SP-SES-007` | A phone call interrupted it. |
| `SP-SES-008` | Replaced by a new stream (a settings change). |
| `SP-SES-009` | Incompatible versions. |
| `SP-SES-010` | The network connection was lost. |
| `SP-SES-011` | The other device does not have that source or output. |
| `SP-SES-012` | It timed out. |
| `SP-SES-013` | Too many pairing attempts from that address. |
| `SP-SES-014` | Another device already feeds that computer's virtual microphone. |

---

Still stuck? Ask in [GitHub Discussions](https://github.com/FlyToRakib/soundpush/discussions) or
[report a problem](https://github.com/FlyToRakib/soundpush/issues/new/choose), quoting the code and
a diagnostics export.
