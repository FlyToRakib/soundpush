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

## Pairing rate limit

Every connection from a key that is not paired counts as a pairing attempt for its source address (an IPv4-mapped
IPv6 address counts as the IPv4 address), whether or not it goes on to send `PairRequest`. More than **5 attempts per
minute** from one address are refused: the connection is closed with `Goodbye(RateLimited)` (stop reason 13) before
any pairing work. The first refusal shows a notice and writes the security log; the dialing device reports "too many
pairing attempts". Refused attempts do not extend the window. At most 256 addresses are tracked (least recently seen
forgotten first). A peer that does not know stop reason 13 sees an ordinary close.

## Security log

The engine keeps a local audit log, `audit.log` (JSON Lines) in the data folder: pairing attempts (with the source
address), successes and refusals, rate limiting, devices forgotten, blocked and unblocked, permission changes (in
Settings or "remember" on a prompt), route approvals and denials, route starts and stops (with who started them), and
connections refused from blocked devices or changed keys (at most once per device per 10 minutes).

- At most 1000 entries from the last 30 days; older entries are dropped on load and compaction.
- No keys, pairing codes or secrets; device names are untrusted labels and are clipped to 128 characters.
- Never uploaded, and not part of diagnostics exports. Both apps show it (Settings → Privacy → Security log) and can
  clear it; clearing leaves a "log cleared" entry.

## Session resume tokens (protocol 1.1)

After a trusted session is established each side sends the other a `SessionTicket`: 32 random bytes from the OS
RNG, held only in memory. On the next connection the peer returns it in `Hello.resume_token`.

- **Checked after authentication and trust.** The token is looked at only once the TLS handshake succeeded and the
  peer's full public key matched the trust store. It never replaces pinning, SAS or trust checks.
- **Bound to the peer's key.** The issuer stores the key it issued the token to; any other key is refused.
- **Single use, 10 minutes.** A token is removed the first time it is presented, valid or not. Issuing a new token
  replaces the previous one for that peer. Forgetting or blocking a device drops its tokens.
- **Bounded.** At most 64 issued and 64 held tokens.
- **What it allows.** For 30 seconds after a successful redemption, a route request for a route that was running and
  paused by the interruption is accepted without a second "Ask" prompt. "Deny" still refuses, permission changes
  still apply, and a new kind of route still asks.

## USB (TLS over TCP)

`adb reverse` makes the phone's loopback port reach the computer's loopback listener. The TLS handshake over it is
the same mutually authenticated handshake as over QUIC (same certificates, verifiers and trust checks), and the
phone pins the computer's device ID when it dials a trusted computer or scans a QR code. Anything on the phone that
can open a loopback socket can reach the listener, which is why it grants nothing without that handshake. The
pairing SAS is derived from the TCP TLS session's exporter exactly as for QUIC.
