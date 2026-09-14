## What and why

<!-- What does this change, and why? Link the issue or ADR (Fixes #123). -->

## How it was tested

<!-- Commands you ran, platforms and devices you tried (e.g. Windows 11 + Pixel 8 on Wi-Fi). -->

## Checklist (Definition of Done, docs/soundpush-final.md §30.2)

- [ ] Tests added or updated; `just core-test` and `just desktop-check` pass (Android: `./gradlew testDebugUnitTest lint`)
- [ ] Docs updated (user guide, ADR, CHANGELOG under **Unreleased**) where behaviour changed
- [ ] Every user-visible string is externalised (`en.json` / `strings.xml`); no hard-coded text
- [ ] Accessibility: labels and roles, keyboard access and visible focus, no colour-only status, works with reduced motion
- [ ] Errors map to the shared taxonomy and say what happened and what to do
- [ ] Logging/diagnostics hooks added where useful; no audio, keys or secrets in logs
- [ ] No new dependency, or its need is explained below (licence compatible with GPL-3.0-or-later)
- [ ] Commits follow Conventional Commits and are signed off (`git commit -s`)

## Security and privacy implications

<!-- Does this touch pairing, transport, permissions, discovery, parsers of network input, the driver,
     the updater, or data leaving the device? Describe the impact or write "None". -->

## Screenshots

<!-- For UI changes: Light and Dark. -->
