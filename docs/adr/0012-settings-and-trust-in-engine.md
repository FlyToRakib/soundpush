# ADR-0012: Settings and trust stored in the engine with one shared schema

- Status: accepted
- Date: 2026-09-14
- Plan: §12.1, §14.1, decision log ADR-0012

## Context

Latency profiles, quality, visibility, permissions and paired devices mean the same thing on every platform, and
the engine enforces most of them. Storing them per platform (Android DataStore, desktop JSON, keychain entries)
would duplicate validation and migrations and let the apps drift apart.

## Decision

- `sp-engine` owns `Settings` (`sp-engine/src/settings.rs`) and the trust store. Apps read them from the engine
  state and write whole settings objects back through one command (`update_settings`).
- Settings are non-secret JSON, written atomically. Unknown fields are ignored and missing fields take defaults, so
  older and newer app versions can read each other's files. Values are clamped on load. A corrupted file is moved
  aside and defaults are used, with a notice.
- The trust store and device identity are encrypted at rest with a key held by the OS keychain (desktop) or Android
  Keystore.
- Platform-only fields live in sub-objects (`desktop`, `mobile`). Only purely local UI preferences may stay in the
  UI (for example the last update-check time in the desktop webview's storage).

## Consequences

- One place to add a setting: `settings.rs`, then the TypeScript type and the Kotlin data class mirror it.
  New fields must have a serde default so existing files keep loading.
- Permission checks happen in the engine, never in UI code.
- Settings files can be included (sanitised) in diagnostics; the trust store never is.
