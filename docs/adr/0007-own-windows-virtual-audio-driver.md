# ADR-0007: SoundPush's own Windows virtual audio driver

- Status: accepted, deferred (blocked on EV certificate and Microsoft attestation signing)
- Date: 2026-09-14
- Plan: §10.7, §13.4, decision log ADR-0007; revised by [ADR-0003](0003-virtual-microphone.md)

## Context

"Use phone as microphone" needs an input device other Windows apps can select. The plan's original decision was a
custom driver shipped on demand. Windows loads kernel audio drivers on Secure Boot PCs only when Microsoft signs
them, which requires an EV code-signing certificate and a Partner Center account. The project does not have them
yet, so ADR-0003 ships the Microsoft-signed VB-CABLE first. This ADR keeps the design of the driver that replaces it.

Alternatives considered: bundling VB-CABLE permanently (third-party branding "CABLE Output", licence dependency,
two endpoints instead of one), user-mode only (impossible: an input endpoint needs a driver), a Rust driver
(`windows-drivers-rs` is not mature for PortCls/WaveRT miniports).

## Decision

- A small C driver based on the Microsoft SysVAD/WaveRT model in `sound-push-desktop/drivers/windows-virtual-audio/`
  exposes one capture endpoint, **SoundPush Microphone** (48 kHz, 1–2 channels), and optionally a
  **SoundPush Speakers** render endpoint for the mute-PC-speakers feature.
- The engine writes PCM into a shared ring buffer mapped once; there are no per-frame IOCTLs. Underruns produce
  silence. The IOCTL surface is open/close/format/stats only, with strict validation. The private device interface
  is ACL-restricted to the interactive user and SYSTEM. The driver notifies the engine when an app opens the mic.
- Installed on demand through an elevated helper (UAC), never by running the app elevated. INF with `PnpLockdown=1`.
- Quality gates before signing: Driver Verifier, Static Driver Verifier/CodeQL, an IOCTL fuzzer, and an
  install/upgrade/uninstall matrix.
- Built and **test-signed** in CI; released only after **attestation signing** (steps in
  [`docs/release-signing.md`](../release-signing.md) §6). The engine prefers "SoundPush Microphone" over
  "CABLE Input", so switching needs no user action.

## Consequences

- Until signing is available, Windows users get VB-CABLE (ADR-0003) and choose "CABLE Output" in their apps.
- Developers can test the driver with `bcdedit /set testsigning on`.
- The project needs donations or sponsorship for the EV certificate (§32); nothing else in the build depends on it.
- A kernel driver is the highest-privilege code SoundPush ships; it gets a specialist review before release (§21.2).
