# Threat model

Scope: the SoundPush desktop app, Android app, shared Rust engine, pairing and transport, discovery, virtual
microphone drivers, and the update and release pipeline. Source: docs/soundpush-final.md §21 (STRIDE summary and
controls), expanded with the current implementation status. Pairing details: [`pairing.md`](pairing.md).

Review this document when pairing, transport, discovery, permissions, the drivers, the updater or the release
workflow change, and before every minor release. Last reviewed: 2026-09-14.

## Assets

| Asset | Why it matters |
|---|---|
| Microphone audio | Eavesdropping on a room or call is the most serious harm SoundPush could enable |
| System and app audio | May contain private calls, media, notifications |
| Device identity private keys | Whoever holds a key can impersonate that device to every device that trusts it |
| Trust store (paired devices and permissions) | Adding an entry grants access; changing permissions can open the microphone |
| Virtual microphone input | Injected audio could be heard by call participants as the user |
| Update channel and signing keys | A forged update runs code on every installed computer |
| Local logs and diagnostics | Can reveal device names and network details |

## Actors

- **Nearby attacker on the same network** (café, office, shared flat, guest Wi-Fi): can sniff, spoof, flood and
  send arbitrary packets. Primary adversary.
- **Malicious or compromised paired device:** has a trusted key; limited by per-device permissions.
- **Network-path attacker for updates:** can intercept or redirect HTTPS to GitHub (e.g. hostile proxy).
- **Supply-chain attacker:** compromises a dependency, a GitHub Action, or a maintainer account.
- **Local unprivileged process or other user** on the same computer: can try to open the driver device or read files.

Out of scope: an attacker with administrator/root access or physical control of an unlocked device, and compromised
operating systems. They can already read audio devices directly.

## Trust boundaries

1. **LAN / USB link ↔ engine:** every packet is untrusted until the QUIC/TLS handshake with a pinned key succeeds.
2. **Engine ↔ UI:** desktop webview talks to Rust only through the Tauri command allowlist; Android UI through UniFFI.
3. **App ↔ OS audio stack and virtual microphone driver.**
4. **App ↔ GitHub** (update manifests and packages; Android release check).
5. **CI ↔ release artifacts** (secrets, third-party actions, dependencies).
6. **App ↔ local disk** (keys, trust store, settings, logs).

## STRIDE analysis

Status: ✅ implemented · 🟡 partly implemented / needs verification · ⏳ planned.

| Threat | Example | Mitigation | Status |
|---|---|---|---|
| **Spoofing** | Rogue device pretends to be "RAKIB-PC" | Key-pinned mutual TLS; device names are untrusted labels; QR pairing proves possession of a one-time secret; code pairing compares a 6-digit SAS derived from the TLS exporter | ✅ (`sp-security`, `sp-transport`) |
| Spoofing | Forged discovery beacon lures a connection to the attacker | Beacons are signed; paired peers verify them; a connection still requires the pinned key | ✅ (`sp-discovery/beacon.rs`) |
| Spoofing | Fake update server | Updater accepts only packages signed with the project minisign key (pubkey in `tauri.conf.json`); HTTPS to GitHub | ✅ |
| **Tampering** | Inject audio into the virtual microphone over the network | Authenticated encryption on all media (QUIC datagrams under TLS 1.3); media accepted only on routes the user allowed | ✅ |
| Tampering | Local process writes into the virtual microphone | VB-CABLE: any local app can play into "CABLE Input" (accepted risk, same as any audio app). Own driver: private device interface with ACL for the interactive user + SYSTEM | 🟡 (accepted today) / ⏳ (own driver) |
| Tampering | Modified trust store or settings file | Trust store and identity encrypted and authenticated at rest; settings are non-secret and sanitised/clamped on load | ✅ |
| Tampering | Modified release artifact | `SHA256SUMS` published with each release; updater signature; optional GPG signature and Authenticode/Developer ID when enabled | ✅ checksums / ⏳ platform signing (docs/release-signing.md) |
| **Repudiation** | "Who used my microphone?" | Microphone permission defaults to "Ask"; tray and notification show the mic-live state; logs record route starts/stops with device and time. Audit log view in Diagnostics | 🟡 logs / ⏳ audit log UI |
| **Information disclosure** | Eavesdrop microphone audio on shared Wi-Fi | Pairing required; TLS 1.3 only (rustls); no plaintext fallback | ✅ |
| Information disclosure | Discovery reveals device names and presence | "Visible to" setting; default "Paired devices only" omits names; "Nobody" stops advertising | ✅ |
| Information disclosure | Logs or diagnostics leak secrets | Logs never contain audio, keys, pairing secrets or full fingerprints; diagnostics export redacts peer addresses and shortens device IDs; nothing uploaded | ✅ |
| Information disclosure | Android backup clones identity | `identity.bin`, `trust.bin`, `storage.key.enc` excluded from backup and device transfer | ✅ |
| Information disclosure | Update check reveals usage | Only a plain HTTPS GET to GitHub; no identifiers; can be turned off | ✅ |
| **Denial of service** | Handshake flooding, pairing spam | QUIC stateless retry; bounded per-peer resources; pairing mode expires after 5 minutes; pairing rate limits (5/min per source) | 🟡 rate limits to verify |
| Denial of service | Oversized or malformed packets crash the engine | Size-limited control messages (64 KiB); media payload checked against codec maximum; parsers return errors, never panic; fuzz targets (`fuzz/`: beacon, control message, frame decoder, media packet, QR payload) | ✅ parsers / 🟡 nightly fuzzing in CI ⏳ |
| **Elevation of privilege** | Malicious packet exploits a parser | Memory-safe Rust; `#![forbid(unsafe_code)]` in `sp-protocol`, `sp-security`, `sp-transport`, `sp-discovery`; unsafe confined to audio backends and FFI | ✅ (add to `sp-engine` ⏳) |
| Elevation of privilege | Webview script calls privileged commands | Strict CSP (no remote content, no `eval`); Tauri capabilities allow only event, app, updater and process-restart APIs; commands validate input; `open_url` only accepts `https://`; the webview never navigates to remote pages | ✅ |
| Elevation of privilege | Driver IOCTL abuse | Minimal IOCTL surface with strict length/range validation; Driver Verifier, SDV and an IOCTL fuzzer before signing | ⏳ (own driver) |
| Elevation of privilege | Installer or driver install runs attacker code | Per-user NSIS install; elevation only for the virtual microphone install via the OS prompt; VB-CABLE package downloaded from vb-audio.com and verified against a pinned SHA-256 before it runs (`virtual_mic.rs`) | ✅ |
| **Supply chain** | Compromised crate, npm package or Action | Lockfiles committed; `cargo deny` (advisories, licences, sources, dependency direction) and `npm audit --audit-level=high` in CI; Dependabot with review; CycloneDX SBOMs per release; secrets only in the release workflow | ✅ / ⏳ pin Actions to SHAs, Gradle dependency verification |
| Supply chain | Stolen updater or store signing key | Private keys never in the repository; held by maintainers outside CI except as GitHub secrets; key rotation procedure documented | 🟡 hardware-backed storage ⏳ |

## Controls summary

- **Transport:** TLS 1.3 only, X25519, AES-GCM/ChaCha20-Poly1305, no downgrade; one QUIC connection carries control
  (stream) and media (datagrams). TLS-over-TCP fallback for ADB/UDP-blocked networks uses the same pinning (⏳).
- **Authorisation:** trust store plus per-device permissions (receive my audio, use my microphone, send audio to me,
  control me), enforced in the engine, never in UI code. Revocation stops routes within one second.
- **Input safety:** size limits, no panics on network input, fuzzing.
- **Local data:** identity and trust store encrypted at rest with a key held by the OS keychain / Android Keystore.
- **Desktop hardening:** capability allowlist, CSP, webview destroyed when hidden, links open in the default browser.
- **Android hardening:** only the launcher activity, Quick Settings tile (`BIND_QUICK_SETTINGS_TILE`) and boot receiver
  are exported; cleartext traffic disallowed by `networkSecurityConfig`; R8 minification in release builds.
- **Updates and releases:** signed update manifests; checksums and SBOMs; draft releases reviewed before publishing.
- **Privacy:** no analytics or crash upload (PRIVACY.md).
- **Process:** private vulnerability reporting (SECURITY.md), two reviewers for `sp-security`, `sp-transport` and the
  driver (CODEOWNERS), external review of pairing and transport before 1.0.

## Residual risks and open items

1. Windows builds are not Authenticode-signed and macOS builds are not notarised yet, so users must bypass
   SmartScreen/Gatekeeper once; checksums help only users who verify them.
2. Preview Android APKs are signed with a public debug key: anyone can build an APK that installs as an update over
   them. Release keystore needed before wide distribution (docs/release-signing.md §4).
3. Any local process can feed VB-CABLE's input. The own driver with an ACL removes this.
4. Pairing and discovery rate limits and the audit log UI need verification/implementation.
5. Nightly fuzzing, Gradle dependency verification, pinned Action SHAs and signed release tags are not enforced yet.
6. External security review of pairing, transport and the driver is required before 1.0 (§21.2).
