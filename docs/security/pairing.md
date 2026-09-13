# Identity, pairing and trust

Implemented in `sp-security` (primitives) and `sp-engine/src/actor.rs` (flows).

## Identity

- Each installation generates an Ed25519 key on first run, stored encrypted
  (ChaCha20-Poly1305) with a 32-byte key held by the OS keystore
  (Windows Credential Manager, macOS Keychain, Linux Secret Service, Android Keystore).
- `Fingerprint` = SHA-256(public key). `DeviceId` = first 16 bytes. Shown to users as `SP-XXXX-XXXX-XXXX`.
- Android backups exclude the identity and trust store, so a restore never clones a device.

## QR pairing (default)

1. Displayer opens pairing mode and shows a compact code: `SP1:` + uppercase base32 of
   `version | device_id (16) | secret (16) | port | up to 3 addresses`.
   - About 75 characters, all in the QR *alphanumeric* set, so the symbol stays small (≈ 33×33 modules) and scans easily.
   - The 128-bit secret is single-use and expires after 5 minutes; **the displayer enforces expiry**
     (it is the only side that can accept the secret), so the expiry time is not encoded.
   - The device name is not encoded; it arrives inside the encrypted handshake (`Hello`).
   - Link-local IPv6 addresses are skipped (they need an interface scope a code can't carry).
2. Scanner dials an address **pinning the device ID** (128-bit SHA-256 prefix of the displayer's key). The TLS handshake
   fails for any other key; producing a different key with the same 128-bit prefix takes ~2¹²⁸ work.
3. Scanner sends `PairRequest{qr_proof = HMAC-SHA256(secret, "soundpush-pair-v2" ‖ scanner_fingerprint ‖ displayer_device_id)}`.
4. Displayer verifies the proof in constant time, consumes the secret, stores the scanner's full key and replies `PairResult{accepted}`.
5. Scanner stores the displayer's full key (taken from the authenticated TLS certificate).

Both directions are authenticated: the scanner by knowing the out-of-band secret, the displayer by the pinned device ID.
Reconnections of trusted devices pin the device ID in the handshake and then require the exact stored public key.

## Code pairing (no camera, accessibility, desktop↔desktop)

Only accepted while pairing mode is open on the receiving device.

1. Devices complete a TLS handshake with unknown keys.
2. Both derive `SAS = SHA-256(exporter("EXPORTER-soundpush-pair-sas") ‖ sorted fingerprints)` → 6 digits.
3. Each user confirms the codes match; each side sends `PairResult`. Keys are stored only when **both** accepted.
   A man-in-the-middle produces different exporter secrets and therefore different codes.

## Trust and permissions

Per trusted device: `receive_my_audio` (Allow), `use_my_microphone` (**Ask**), `send_audio_to_me` (Allow),
`control_me` (Allow), plus auto-connect, block, alias. Permissions are enforced by the engine on every
route request and re-checked when changed (revocation stops routes immediately).

A connection from an untrusted key is closed unless it is completing a pairing flow.
Forgetting or blocking a device terminates its sessions.
