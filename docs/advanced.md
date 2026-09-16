# Advanced settings

SoundPush decides everything on this page by itself, and the defaults are right on almost every
computer. The switches exist so that support can ask you to change one when a driver on your
machine misbehaves — they are the modern equivalent of AudioRelay's "EventSync" and "continuous
capture" options (plan §4.3, §24.7).

They live in **Settings → Help & support → Advanced**. The section is folded away, and folds open
by itself whenever something is not at its default, so nothing stays changed without you knowing.
**Back to defaults** puts everything back in one click.

If a change here does not fix what you are chasing, put it back. Leaving one on costs battery,
bandwidth or latency for no benefit.

## Continuous capture

**Default: off.**

While the source is completely silent, SoundPush stops sending audio and tells the other side to
play silence instead (DTX, plan §15.2). It saves bandwidth and battery, and the receiver never
hears a gap.

Turning **Continuous capture** on keeps encoding and sending during silence. It costs the full
bitrate all the time. Try it when:

- a receiver's audio driver stalls or clicks when a stream goes quiet and comes back, or
- another program monitors the stream and treats silence as "the device is gone".

The setting applies to streams this computer sends. Lossless (PCM) already sends every frame, so
it changes nothing there.

## Keep audio devices open

**Default: off.**

A minute after the last stream stops, SoundPush lets go of everything it holds for audio and only
the network side stays awake (plan §13.5). Starting a stream reopens what it needs, which takes a
fraction of a second on normal hardware.

Turn **Keep audio devices open** on when a device is slow or unreliable to reopen — some USB
interfaces and Bluetooth headsets take seconds, and a few virtual cables fail the first time after
they have been idle. The cost is that the device stays claimed, so other programs may see it as
in use.

## Real-time audio priority

**Default: on.**

Capture and playback run on threads the operating system provides, and SoundPush asks for
real-time priority on them the first time each one delivers audio: MMCSS "Pro Audio" on Windows,
the thread time-constraint policy on macOS, and `SCHED_RR` (or a better `nice` value) on Linux.
That is what keeps a game or a compile from causing dropouts (plan §13.3, §8.3 "CPU starvation").

If the system refuses — a container, a policy, a machine without the right limits — SoundPush
carries on at normal priority and says so in the detailed log. Nothing fails.

Turn it **off** only if a virtual audio driver misbehaves under real-time priority. Threads that
are already running keep the priority they were given until the stream stops.

## Related things that are not here

- **Windows firewall.** "Allow SoundPush" runs Windows' own tools through one administrator
  prompt. Everything it runs is named by its full path under the real system directory, and the
  only values that reach the command line are this program's path and, when you ask for the
  network to be moved to Private, adapter ids checked to be exactly `{8-4-4-4-12}` hexadecimal.
  The plan (§13.6) asks for a separate signed helper program instead. SoundPush installs per user,
  so that helper would sit in a folder anything running as you can write to, while being the
  program an administrator approves — a worse position than the one above. It waits for a
  per-machine install.
- **Keep window in memory for instant reopen** and **Make SoundPush the default input and output
  while active** are ordinary settings in Settings → General, not overrides.
- **Detailed logging** is in Settings → Help & support, next to the diagnostics export.
