# ADR-0019: Releases and updates without paid certificates

- Status: accepted
- Date: 2026-09-14
- Plan: §32, §36.1; related: ADR-0003, ADR-0007, ADR-0013

## Context

The plan expects signed installers, a signed updater and store releases. Windows Authenticode (especially EV),
Apple Developer ID and Microsoft driver attestation cost money the project does not have yet (ADR-0013). Users
still need trustworthy downloads and automatic updates now.

## Decision

- **Updater:** the Tauri updater with a project **minisign** key (free). Only the public key is in
  `tauri.conf.json`; the private key and password are kept outside the repository and provided to the release
  workflow as `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The app downloads `latest.json`
  from GitHub Releases, verifies signatures, and installs only when the user chooses "Restart to install".
- **Distribution:** GitHub Releases, created as drafts by `.github/workflows/release.yml` on `v*` tags, with
  `SHA256SUMS` and CycloneDX SBOMs. A maintainer tests and publishes the draft.
- **Platform signing today:** Windows installers unsigned; macOS ad-hoc signed with entitlements; Android APK signed
  with the public debug key and labelled `debug-key-signed` in the file name. Local builds never need any key
  (`createUpdaterArtifacts` is enabled only in the release workflow).
- **Prepared paid signing:** workflow steps for Authenticode, Developer ID + notarisation, the Android release
  keystore and GPG checksums run only when their secrets exist. Instructions: `docs/release-signing.md`.
- Windows virtual microphone stays VB-CABLE until the own driver can be attestation-signed (ADR-0003, ADR-0007).

## Consequences

- Automatic updates are secure against tampering from day one, independent of platform code signing.
- First-time installation shows SmartScreen / Gatekeeper warnings; the user guide explains them.
- Preview APKs cannot be upgraded in place by release-key builds later; users reinstall once.
- Losing the minisign private key breaks automatic updates for all installed copies; it must be backed up offline.
- Staged rollouts (§32) are not possible with a single static `latest.json`; every published release reaches all
  users. A later channel/rollout service can replace the endpoint without changing the app's verification.
