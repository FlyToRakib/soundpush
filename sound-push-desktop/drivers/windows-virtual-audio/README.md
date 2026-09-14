# SoundPush Virtual Audio (Windows driver)

SoundPush's own Windows virtual microphone: a kernel-mode PortCls/WaveRT audio driver.
It is the Windows counterpart of the macOS AudioServerPlugIn in `../macos-virtual-mic`.

> **Status: built and test-signed in CI, not used by the app.** VB-CABLE remains the active
> Windows virtual microphone. Windows loads this driver on normal PCs only after Microsoft
> **attestation signing**, which needs an EV code-signing certificate and a Partner Center
> account. That step is postponed together with code signing
> (see `docs/virtual-microphone.md` §4 and `docs/implementation-status.md`).

## What it does

One device, **SoundPush Virtual Audio**, with two endpoints:

| Endpoint (as Windows shows it) | Direction | Who uses it |
|---|---|---|
| `SoundPush Microphone Feed (SoundPush Virtual Audio)` | playback | the SoundPush engine plays the phone's microphone into it |
| `SoundPush Microphone (SoundPush Virtual Audio)` | recording | Meet, Zoom, Discord, OBS, recorders |

Audio written to the feed comes out of the microphone about 30 ms later. When nothing is fed,
apps record silence. Formats: 48 kHz, mono or stereo, 16/24/32-bit integer PCM; the Windows
audio engine converts to and from its float mix format.

## Architecture

```
SoundPush engine ──WASAPI──► "SoundPush Microphone Feed"         "SoundPush Microphone" ──WASAPI──► Meet / Zoom
                                   │                                        ▲
                    WaveRT render filter ─ topology (line-level jack)       │
                                   │ render stream timer                    │ capture stream timer
                                   ▼                                        │
                          ┌──────────────── cable: lock-free ring buffer ───┘
                          │   stereo 32-bit, 16384 frames, one writer / one reader
                          └───────────────── WaveRT capture filter ─ topology (microphone jack)
```

| File | Role |
|---|---|
| `src/adapter.cpp` | `DriverEntry`, `AddDevice`, `StartDevice`; the adapter object (owns the cable, implements `IAdapterPowerManagement`); installs the four subdevices and their physical connections |
| `src/wavert.cpp` | WaveRT miniport and stream: format validation, cyclic buffer allocation, clock emulation, event notifications |
| `src/topology.cpp` | Topology miniport: jack pins, endpoint names, `KSPROPERTY_JACK_DESCRIPTION`/`2` |
| `src/cable.h`, `src/cable.cpp` | The ring buffer and sample conversion |
| `SoundPushVirtualAudio.inf` | DCH-compliant INF: MEDIA class, KS interfaces, endpoint names (HKR `MediaCategories`), `PnpLockdown=1` |
| `scripts/Install-SoundPushDriver.ps1` | Test-computer install/uninstall (`pnputil` + SwDevice API, no devcon) |
| `scripts/Test-DriverBuild.ps1` | Build and package checks without the WDK Visual Studio component |

Design notes:

- **Clock.** No hardware: each stream's position advances at exactly 48 000 frames per second of
  the performance counter. A high-resolution `EX_TIMER` (and every `GetPosition` call) moves
  the frames between the last and current position. Render and capture share this clock, so
  the ring stays at a constant fill level with no drift correction.
- **Ring buffer.** Single-producer/single-consumer with 64-bit frame counters updated through
  `Interlocked*` calls; each endpoint allows one stream, so there is exactly one writer and one
  reader. The reader keeps 30 ms queued, primes again after an underrun (silence meanwhile), and
  skips stale audio when more than 100 ms piles up. Audio is discarded while nobody records.
- **Event mode.** Streams implement `IMiniportWaveRTStreamNotification`; the timer fires at each
  notification boundary so event-driven clients are woken on time. Polling clients get a 10 ms timer.
- **Safety.** Every client format, buffer size and notification count is validated; buffers are
  clamped to 10 ms–2 s and zeroed; partial MDL allocations are rejected; buffer teardown detaches
  under the stream lock before unmapping; the stream destructor deletes its timer and waits for a
  running callback. Code that holds spin locks or runs at `DISPATCH_LEVEL` is non-paged; the rest
  is `PAGE`/`INIT`. All pool is `ExAllocatePool2` (zeroed, NX). Warnings are errors (`/W4 /WX`),
  and `/analyze` is clean.
- **Power.** PortCls forwards D-state changes to the adapter; outside D0 the streams move silence.
- **Endpoint form factors.** The feed's jack is line level, not speakers, so it looks like a cable
  output. The capture jack is a microphone.
- **Deviation from `docs/soundpush-final.md` §13.4.** That design had the engine write into a shared
  section through private IOCTLs. This driver uses a render endpoint instead, so the engine
  works through the same WASAPI path it already uses for VB-CABLE: no engine change, and there is
  no private IOCTL surface to secure or fuzz. The "microphone opened/closed" notification from
  §13.4 is a follow-up. The engine can watch audio sessions on the capture endpoint without a
  driver change.
- **Language.** C-style C++ (no exceptions, RTTI, STL or global constructors), because PortCls
  interfaces are C++ COM interfaces. The code is original, written against the documented
  PortCls/WaveRT APIs; it contains no SysVAD code.

## Build

The WDK and SDK come from NuGet (`packages.config`, restored into `packages/`, git-ignored).

### MSBuild (CI, or a PC with Visual Studio's WDK component)

Requirements: Visual Studio 2022/2026 with the C++ tools, the Spectre-mitigated libraries,
the ARM64 tools for ARM64, and the **Windows Driver Kit** component, which provides the
`WindowsKernelModeDriver10.0` toolset.

```powershell
cd sound-push-desktop\drivers\windows-virtual-audio
nuget restore packages.config -PackagesDirectory packages
msbuild SoundPushVirtualAudio.vcxproj /p:Configuration=Release /p:Platform=x64
msbuild SoundPushVirtualAudio.vcxproj /p:Configuration=Release /p:Platform=ARM64
```

Output: `build\<Platform>\Release\SoundPushVirtualAudio.sys` and the stamped `.inf`.

### Without the WDK Visual Studio component (no admin needed)

```powershell
.\scripts\Test-DriverBuild.ps1 -Analyze
```

This builds x64 with `cl`/`link` using the WDK's kernel-mode settings, then runs `stampinf`,
`infverif /w`, `inf2cat` and `ApiValidator` into `build\direct\x64\Release\package`. It does not sign.

### CI

`.github/workflows/windows-driver.yml` runs on `windows-2025-vs2026`. It builds Release x64 and
ARM64 with code analysis (warnings as errors), runs `infverif` and `ApiValidator`, creates the
catalog, and test-signs with a certificate generated in the job. The private key is never
exported and is deleted at the end. The job then uploads the artifact
**`SoundPush-Windows-Driver-TestSigned`**: `x64/` and `ARM64/` (`.sys`, `.inf`, `.cat`, `.pdb`),
`SoundPushTestSigning.cer`, `Install-SoundPushDriver.ps1` and `INSTALL.txt`.

## Install on a test computer

Never on a PC you rely on: test-signing mode lowers protection against unsigned kernel code.
Use a VM or a spare PC with Secure Boot off.

```powershell
# elevated PowerShell, in the unzipped artifact
bcdedit /set testsigning on          # then restart
certutil -addstore Root SoundPushTestSigning.cer
certutil -addstore TrustedPublisher SoundPushTestSigning.cer
.\Install-SoundPushDriver.ps1 -PackagePath .\x64
```

The script adds the package to the driver store (`pnputil /add-driver /install`) and creates
the software device `SWD\SoundPush\SoundPushVirtualAudio` with `SwDeviceCreate`. The device
persists across restarts. The INF also matches `Root\SoundPushVirtualAudio`, for tools that
create root-enumerated devices.

What to check:

1. Settings → System → Sound lists both endpoints, and Device Manager shows no error.
2. Play a tone into the feed; Sound Recorder on "SoundPush Microphone" records it. Stop the tone and the recording is silent.
3. Try mono/stereo and 16/24-bit formats (Sound control panel → Advanced), exclusive mode, and event-driven clients.
4. Sleep/resume while streaming; disable and enable the device.
5. Driver Verifier (`verifier /standard /driver SoundPushVirtualAudio.sys`) during all of the above.
6. Check whether Windows ever makes the feed the default playback device.

### Uninstall

```powershell
.\Install-SoundPushDriver.ps1 -Uninstall
certutil -delstore Root "SoundPush Test Driver Signing"
certutil -delstore TrustedPublisher "SoundPush Test Driver Signing"
bcdedit /set testsigning off         # then restart
```

## How the app will use it

No engine change is needed. `sound-push-desktop/src-tauri/src/hooks.rs` (`VIRTUAL_CABLES`)
lists `SoundPush Microphone Feed` first, with `SoundPush Microphone (SoundPush Virtual Audio)`
as its recording side, ahead of VB-CABLE. Once the driver is installed, the engine feeds the
phone microphone into it and tells users to pick "SoundPush Microphone". Without it,
VB-CABLE works exactly as before. The endpoint names are INF strings; if they change,
`hooks.rs` and its tests must change too.

Switching the installer from VB-CABLE to this driver waits for attestation signing:

1. EV certificate + Partner Center.
2. HLK/attestation submission of the CI-built package.
3. Installer step plus an Audio page status.
