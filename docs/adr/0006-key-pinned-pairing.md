# ADR-0006: Key-pinned pairing via QR secret or TLS-exporter SAS

- Status: accepted
- Date: 2026-09-14
- Plan: §18, decision log ADR-0006

## Context

A paired device can use the microphone and hear system audio, so pairing is the security root of SoundPush. It
must resist a man-in-the-middle on shared Wi-Fi, work offline without accounts or servers, and have a path for
users who cannot scan a QR code (no camera, blind users, desktop ↔ desktop). Device names on the network are
attacker-controlled.

Alternatives considered: password/PIN PAKE (users pick weak PINs, extra protocol), no pairing (unacceptable for a
microphone), cloud accounts (needs a backend, contradicts ADR-0013).

## Decision

- Every installation has an Ed25519 identity; the **key**, not the name or certificate, is what is trusted.
  `DeviceId` = first 16 bytes of SHA-256(public key).
- **QR pairing (primary):** the desktop shows a QR with addresses, port, its fingerprint and a single-use 128-bit
  secret valid for 5 minutes. The phone connects, checks the fingerprint, and proves possession of the secret with
  an HMAC over both fingerprints. The camera is the out-of-band channel, so no code comparison is needed.
- **Code pairing (accessible alternative):** both sides connect with unknown certificates in pairing mode only,
  derive a 6-digit short authentication string from the TLS exporter over both fingerprints, and the user confirms
  the codes match on both devices. A mismatch aborts.
- After pairing, every connection uses mutual TLS with the pinned keys (ADR-0005). Per-device permissions are
  stored with the trust entry (§18.3); the microphone defaults to "Ask".
- Pairing mode exits after 5 minutes and is rate-limited.

## Consequences

- MITM on the LAN cannot pair without the QR secret or without producing a visibly different code.
- Pairing works with no internet access and no SoundPush servers.
- Losing the identity key (reinstall, damaged key store) means pairing again; the app says so
  (`notice.identityReset`, `notice.peerForgotUs`).
- Details and message formats: [`docs/security/pairing.md`](../security/pairing.md); threats:
  [`docs/security/threat-model.md`](../security/threat-model.md).
