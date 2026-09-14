# Release signing: what is signed today, and how to turn on paid signing

SoundPush releases are built by `.github/workflows/release.yml` when a `v*` tag is pushed. Everything in this
repository builds and runs **without paid certificates**. Paid signing is prepared: each step below activates when
its GitHub secrets exist and is skipped otherwise. No workflow change is needed to turn it on.

Add secrets under GitHub → repository **Settings → Secrets and variables → Actions → New repository secret**.

## Status

| What | Today (free) | With paid signing |
|---|---|---|
| Desktop update manifests (`latest.json`) | **Signed** with the project's minisign key; the app refuses unsigned updates | unchanged |
| Windows installer (`*-setup.exe`) | Not Authenticode-signed; SmartScreen shows "Windows protected your PC" | Authenticode (OV/EV certificate or Azure Trusted Signing) |
| macOS app and DMG | Ad-hoc signed (`signingIdentity: "-"`); Gatekeeper needs "Open Anyway" | Developer ID + notarised + stapled |
| Linux packages | Checksums only | `SHA256SUMS.asc` signed with the project GPG key (free) |
| Android APK | Signed with the shared **debug** key, file named `…_debug-key-signed.apk` | Release keystore; Play App Signing for the store |
| Windows virtual microphone | Microsoft-signed VB-CABLE (VB-Audio) | SoundPush's own driver, attestation-signed |
| Checksums and SBOMs | `SHA256SUMS`, CycloneDX SBOMs for Rust and the desktop UI | unchanged |

## 1. Desktop updater key (free, already set up)

The updater verifies every update against the public key in `sound-push-desktop/src-tauri/tauri.conf.json`
(`plugins.updater.pubkey`). The matching private key signs the update bundles in the release workflow.

- The key pair was generated with `tauri signer generate`. The private key and its password are stored **outside
  the repository** on the maintainer's machine (`%USERPROFILE%\.soundpush\updater\soundpush-updater.key` and
  `soundpush-updater.password`). Keep an offline backup (for example an encrypted USB drive or a password manager).
  **If the key is lost, installed apps can never update automatically again**; users would have to reinstall.
- GitHub secrets:

  | Secret | Value |
  |---|---|
  | `TAURI_SIGNING_PRIVATE_KEY` | The full contents of `soundpush-updater.key` |
  | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The contents of `soundpush-updater.password` |

- Without these secrets the release still builds, but without updater bundles or `latest.json`, and the run shows a
  warning annotation.
- **Rotating the key:** generate a new pair, ship a release signed with the *old* key whose `tauri.conf.json`
  contains the *new* public key, then switch the secrets. Never replace the public key and the secret in one step.

## 2. Windows Authenticode

Removes the SmartScreen warning (immediately with an EV certificate; OV certificates build reputation over time).

1. Buy a code-signing certificate from a CA (OV or EV) or set up Azure Trusted Signing. EV certificates are issued on
   a hardware token or cloud HSM and cannot be exported as a file; for those use the `signCommand` option below.
2. For an exportable certificate, export it as `.pfx` with a password and encode it:
   `[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx")) | Set-Clipboard`
3. Add secrets:

   | Secret | Value |
   |---|---|
   | `WINDOWS_CERTIFICATE` | Base64 of the `.pfx` |
   | `WINDOWS_CERTIFICATE_PASSWORD` | The `.pfx` password |

4. The release workflow imports the certificate and passes `bundle.windows.certificateThumbprint`,
   `digestAlgorithm: sha256` and `timestampUrl: http://timestamp.digicert.com` to `tauri build`, which signs
   `SoundPush.exe` and the NSIS installer.
5. For EV tokens / Azure Trusted Signing, replace the import step with `bundle.windows.signCommand`
   (for example `trusted-signing-cli -e <endpoint> -a <account> -c <profile> %1`) and add that tool's credentials as
   secrets.
6. Verify on a clean PC: right-click the installer → Properties → Digital Signatures.

## 3. Apple Developer ID and notarisation

Needed so macOS opens SoundPush without "Open Anyway", and to distribute the SoundPush Microphone HAL plug-in.

1. Join the Apple Developer Program (paid, yearly).
2. In Xcode or developer.apple.com create a **Developer ID Application** certificate. Export it from Keychain Access
   as `.p12` with a password; `base64 -i cert.p12 | pbcopy`.
3. Create an **app-specific password** at appleid.apple.com (for notarisation) and note your Team ID.
4. Add secrets:

   | Secret | Value |
   |---|---|
   | `APPLE_CERTIFICATE` | Base64 of the `.p12` |
   | `APPLE_CERTIFICATE_PASSWORD` | The `.p12` password |
   | `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID1234)` |
   | `APPLE_ID` | Apple ID e-mail used for notarisation |
   | `APPLE_PASSWORD` | The app-specific password |
   | `APPLE_TEAM_ID` | The 10-character Team ID |

5. When `APPLE_CERTIFICATE` exists the workflow exports these variables and overrides
   `bundle.macOS.signingIdentity` (ad-hoc `"-"` in `tauri.macos.conf.json`). `tauri build` then signs with the
   hardened runtime and `Entitlements.plist`, notarises and staples the app and DMG.
6. The embedded `SoundPushMicrophone.driver` is signed as part of the bundle. Check it after the first signed build:
   `codesign --verify --deep --strict --verbose=2 SoundPush.app` and `spctl -a -vv SoundPush.app`.
7. After switching, users who allowed the microphone for the ad-hoc build must allow it once more (the signature
   changes): `tccutil reset Microphone net.soundpush.desktop`.

## 4. Android release keystore and Google Play upload key

The debug key in `sound-push-mobile/android/debug.keystore` is public and must never sign store builds.

1. Create the release key **once** and keep it forever (every future update must use it):
   ```bash
   keytool -genkeypair -v -keystore soundpush-release.jks -alias soundpush \
     -keyalg RSA -keysize 4096 -validity 10000
   ```
   Store the keystore and passwords in a password manager plus an offline backup.
2. Add secrets:

   | Secret | Value |
   |---|---|
   | `ANDROID_KEYSTORE` | `base64 -w0 soundpush-release.jks` |
   | `ANDROID_KEYSTORE_PASSWORD` | Keystore password |
   | `ANDROID_KEY_ALIAS` | `soundpush` |
   | `ANDROID_KEY_PASSWORD` | Key password |

3. When `ANDROID_KEYSTORE` exists, the workflow re-signs the release APK with `apksigner` (v2 + v3 schemes), verifies
   it, and names it `SoundPush_<version>_android.apk` instead of `…_debug-key-signed.apk`.
4. **Google Play:** enable **Play App Signing**. Google holds the app-signing key; you upload builds signed with an
   *upload key*. Either use the release key above as the upload key, or create a separate upload key the same way and
   register it in Play Console → Setup → App signing. Build an AAB with `./gradlew bundleRelease` and sign it with the
   upload key (`jarsigner` or a Gradle `signingConfig` fed from the same secrets).
5. **F-Droid** builds from source and signs with F-Droid's key, or reproducibly verifies the GitHub APK if the
   release keystore is used consistently. Keep builds reproducible (no timestamps in the APK, pinned toolchains).
6. **Important:** an APK signed with the release key cannot update an installed debug-key build. Users of preview
   APKs uninstall once when the first release-key build ships; say so in the release notes.

## 5. Linux package signature (free)

1. Create a project key: `gpg --full-generate-key` (ed25519, no expiry or a long one), publish the public key in the
   repository (`docs/soundpush-release-key.asc`) and on keys.openpgp.org.
2. Add secrets `LINUX_GPG_PRIVATE_KEY` (`gpg --armor --export-secret-keys <id>`) and `LINUX_GPG_PASSPHRASE`.
3. The publish job then writes `SHA256SUMS.asc` (detached signature of `SHA256SUMS`). Users verify with
   `gpg --verify SHA256SUMS.asc SHA256SUMS && sha256sum -c --ignore-missing SHA256SUMS`.
4. Signing the `.deb`/`.rpm` files themselves (and an apt/yum repository) can follow later with `dpkg-sig`/`rpm --addsign`.

## 6. SoundPush's own Windows driver (attestation signing)

Replaces VB-CABLE with "SoundPush Microphone" (ADR-0003 stage 2, ADR-0007). Windows loads kernel drivers on
Secure Boot PCs only when Microsoft signs them.

1. Obtain an **EV code-signing certificate** (required to register with the Hardware Developer Program).
2. Register a company or individual account in **Microsoft Partner Center → Hardware Dev Center** and sign the
   legal agreements with the EV certificate.
3. Build the driver in Release for x64 (and ARM64 later), run Driver Verifier, Static Driver Verifier/CodeQL and the
   IOCTL fuzzer (docs/soundpush-final.md §29).
4. Package `SoundPushMicrophone.inf`, `.sys` and `.cat` into a CAB (`makecab /f driver.ddf`) and sign the CAB with the
   EV certificate (`signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /sha1 <thumbprint> driver.cab`).
5. Hardware Dev Center → **Submit new hardware** → choose **Attestation signing**, select Windows 10/11 x64, upload the
   CAB. Microsoft signs it in minutes to hours; download the signed package.
6. Commit nothing from the signed package that is secret (the signed `.sys`/`.cat` are public); place it where the
   installer expects it and switch the NSIS installer from VB-CABLE to the SoundPush driver (`installerHooks`).
7. Test install/upgrade/uninstall on Windows 10 22H2 and 11 with Secure Boot on. Attestation-signed drivers are for
   Windows 10/11 client only; Windows Server would need full HLK certification.
8. Later automation: the Hardware Dev Center submission API can be scripted from CI with an Azure AD app; until then
   submissions are manual and nothing is needed in GitHub secrets.

## Release checklist

1. Update versions: `sound-push-desktop/src-tauri/tauri.conf.json` (`version`), workspace `Cargo.toml`,
   `sound-push-desktop/ui/package.json`, Android `versionName`/`versionCode` in `app/build.gradle.kts`.
2. In `CHANGELOG.md` rename **Unreleased** to `## [x.y.z] - YYYY-MM-DD` and add a new empty **Unreleased**.
3. Commit, then tag and push: `git tag -s vX.Y.Z -m "SoundPush X.Y.Z" && git push origin vX.Y.Z`.
4. Wait for **Release** in GitHub Actions. It creates a **draft** release with all files.
5. Download and smoke-test the installers on each platform; check `SHA256SUMS`.
6. Publish the draft. Only a published release is visible to the updater (`releases/latest/download/latest.json`).
7. To roll back, publish a new patch release with the previous code (the updater never downgrades on its own).

Staged rollouts (10 % → 50 % → 100 %, §32) are not implemented yet; every published release reaches all users.
