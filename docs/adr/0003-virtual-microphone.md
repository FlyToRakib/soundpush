# ADR-0003: Virtual microphone — built-in on macOS, bundled VB-CABLE then own signed driver on Windows

- Status: accepted (revised)
- Date: 2026-09-13

## Context

"Phone as PC microphone" needs an OS input device that other apps (Meet, Zoom, recorders) can
select. Only an audio driver can add one.

- **macOS:** an AudioServerPlugIn runs in user space inside `coreaudiod`. It can be built with the
  Command Line Tools and installed with an administrator prompt, and needs no special signing for local use.
- **Windows:** kernel audio drivers load on Secure Boot PCs only when Microsoft-signed. Attestation
  signing requires an EV code-signing certificate, which the project does not have. AudioRelay's own
  "Virtual Mic for AudioRelay" driver is Microsoft-signed, which is how they avoid a third-party install.
- **VB-CABLE** is free, Microsoft-signed, and VB-Audio's licence allows bundling it (including silent
  installation) with free or commercial apps, as long as it stays identifiable as VB-Audio's donationware.
- Users must never have to find and install extra software separately.

## Decision

1. **macOS (done):** SoundPush's own "SoundPush Microphone" AudioServerPlugIn
   (`sound-push-desktop/drivers/macos-virtual-mic`), embedded in the app and installed from the Audio page.
2. **Windows stage 1 (now):** the SoundPush installer bundles the official VB-CABLE package and installs
   it silently in the same run, followed by one restart prompt. It shows VB-Audio's credit line and skips
   the step if VB-CABLE is already installed.
3. **Windows stage 2 (in parallel):** build our own "SoundPush Microphone" WaveRT driver in the repo and
   test-sign it in CI. Once the project obtains an EV certificate and Microsoft attestation signing, the
   installer ships this driver instead of VB-CABLE.
4. **Linux:** the engine creates a PipeWire "SoundPush Microphone" source (no driver).
5. **One detection path:** the engine uses whichever device is present, preferring "SoundPush Microphone",
   then "CABLE Input". A speaker is never used as a virtual microphone.

## Consequences

- Windows users get a working virtual microphone from a single installer today, with no separate install.
- The Windows device is named "CABLE Output" until stage 2 ships; the Audio page tells users what to select.
- Before public release, obtain written confirmation from VB-Audio for bundling.
- Stage 2 release remains blocked on the EV certificate (tracked in `docs/implementation-status.md`).
- Details and research: `docs/virtual-microphone.md`.
