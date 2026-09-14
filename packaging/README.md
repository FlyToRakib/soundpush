# Store and package-manager manifests

SoundPush is distributed through GitHub Releases, winget, Homebrew, Flathub, F-Droid and Google Play
(docs/soundpush-final.md §32, §36.1). The files here are **templates** with `@PLACEHOLDER@` values. For every stable
release, the release workflow fills them in and uploads them as the `store-manifests` artifact; you can also do it by
hand:

```bash
gh release download v0.2.0 --pattern SHA256SUMS
node tools/release/fill-manifests.mjs --version 0.2.0 --sums SHA256SUMS --out store-manifests
```

The script takes the SHA-256 of each package from `SHA256SUMS`, the Android `versionCode` from
`sound-push-mobile/android/app/build.gradle.kts`, and uses the `v<version>` tag as the source commit. A manifest whose
package is missing (for example no ARM64 installer) is skipped with a warning; `--strict` turns that into an error.
None of these stores needs an account in this repository or a secret in GitHub.

| Store | Template | Submit to |
|---|---|---|
| winget | `winget/` (version, installer, locale) | Pull request to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) |
| Homebrew | `homebrew/soundpush.rb` | A tap (`FlyToRakib/homebrew-soundpush`), later homebrew/homebrew-cask |
| Flathub | `flatpak/` | New app repository on Flathub |
| F-Droid | `fdroid/net.soundpush.android.yml` + fastlane texts | Merge request to [fdroiddata](https://gitlab.com/fdroid/fdroiddata) |
| Google Play | fastlane texts | Play Console (or `fastlane supply`) |

## winget

1. Fill the templates, then check them on Windows: `winget validate --manifest store-manifests/winget/manifests/s/SoundPush/SoundPush/<version>`.
2. Test the install: `winget install --manifest <that folder>` (needs `winget settings --enable LocalManifestFiles` once).
3. Copy the folder into a fork of microsoft/winget-pkgs at the same path and open a pull request. `wingetcreate submit`
   does steps 3 in one go.

The installer is per-user NSIS, so winget installs without administrator rights. Until the installer is Authenticode
signed (docs/release-signing.md), winget's validation pipeline may flag SmartScreen reputation; that does not block
the submission.

## Homebrew

The official homebrew/homebrew-cask repository only accepts apps that are signed and notarised. Until the Apple
Developer ID signing in docs/release-signing.md is active, publish the cask in a project tap:

1. Create the repository `FlyToRakib/homebrew-soundpush` with the filled `Casks/s/soundpush.rb`.
2. Users install with `brew install --cask flytorakib/soundpush/soundpush`.
3. Check locally: `brew audit --cask --new flytorakib/soundpush/soundpush` and `brew install --cask …`.

`auto_updates true` tells Homebrew that the app updates itself (Settings → About). `zap` removes the data folder and
the SoundPush Microphone HAL plug-in.

## Flathub

The manifest repackages the release `.deb` into the GNOME runtime (WebKitGTK, libpulse and PipeWire are included) and
adds libayatana-appindicator for the tray from flathub/shared-modules.

Permissions and why:

| Permission | Needed for |
|---|---|
| `--share=network` | pairing, mDNS discovery, streaming, update notice |
| `--socket=pulseaudio`, `--filesystem=xdg-run/pipewire-0` | playback, system-audio capture, the virtual microphone |
| `--talk-name=org.kde.StatusNotifierWatcher` | tray icon |
| `--talk-name=org.freedesktop.secrets` | device identity key in the keyring |
| `--socket=wayland`, `--socket=fallback-x11`, `--device=dri`, `--share=ipc` | the window |

Submitting:

1. Fork [flathub/flathub](https://github.com/flathub/flathub), branch `new-pr`, add `net.soundpush.desktop.yml`, the
   metainfo file and `shared-modules` as a git submodule, and open the pull request.
2. Build and lint locally first:
   ```bash
   flatpak run org.flatpak.Builder --force-clean --user --install --install-deps-from=flathub build net.soundpush.desktop.yml
   flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest net.soundpush.desktop.yml
   flatpak run --command=flatpak-builder-lint org.flatpak.Builder appstream net.soundpush.desktop.metainfo.xml
   ```
3. Before submitting, add screenshots to the metainfo file (Flathub requires them) and confirm control of
   `soundpush.net` or switch the app id to `io.github.flytorakib.SoundPush`.

Known Flatpak limitations to solve before submission: launch at sign-in writes `~/.config/autostart` inside the
sandbox (use the Background portal instead), preventing sleep uses `systemd-inhibit`, which is not in the runtime (use
the Inhibit portal), and in-app updates are turned off inside Flatpak (Flathub updates the app).

## F-Droid

SoundPush uses no proprietary libraries (no Google Play Services, Firebase or ML Kit), so it qualifies for the main
F-Droid repository. F-Droid builds from source and signs with its own key.

1. Fork fdroiddata, copy the filled `metadata/net.soundpush.android.yml`, and run
   `fdroid readmeta && fdroid lint net.soundpush.android && fdroid build -v -l net.soundpush.android`.
2. The recipe installs Rust through the rustup srclib, generates the Kotlin bindings and native libraries with
   cargo-ndk, removes the debug signing of release builds, and builds the unsigned release APK.
3. F-Droid reads the title, descriptions, changelogs and icon from
   `sound-push-mobile/android/fastlane/metadata/android/<locale>/`. Add `changelogs/<versionCode>.txt` (500
   characters at most) for every release.
4. `UpdateCheckMode: Tags` picks up new `vX.Y.Z` tags automatically; beta tags are ignored.
5. Later: reproducible builds, so F-Droid can publish the APK signed with the project key (`AllowedAPKSigningKeys`).

## Google Play

The listing texts are in `sound-push-mobile/android/fastlane/metadata/android/en-US/` in the layout
[fastlane supply](https://docs.fastlane.tools/actions/supply/) uses: `title.txt` (30 characters),
`short_description.txt` (80), `full_description.txt` (4000), `changelogs/<versionCode>.txt` (500) and
`images/icon.png` (512 × 512). Still needed before the first upload: a feature graphic (1024 × 500) and phone
screenshots in `images/featureGraphic.png` and `images/phoneScreenshots/`.

Play Console also needs: the privacy policy URL (`https://flytorakib.github.io/soundpush/privacy/`), the data safety
form ("no data collected or shared"), foreground service declarations with a short video for `mediaProjection`,
`microphone` and `connectedDevice`, an AAB signed with the upload key (docs/release-signing.md §4), and staged rollout
(10 % → 50 % → 100 %) on the production track. Beta versions go to the open testing track.
