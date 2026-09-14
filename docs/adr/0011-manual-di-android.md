# ADR-0011: Manual dependency injection on Android

- Status: accepted
- Date: 2026-09-14
- Plan: §14.1, decision log ADR-0011

## Context

The Android app has a small object graph: one engine facade, one streaming service, a handful of screens. Hilt adds
annotation processing (slower builds), generated code and a learning curve; Koin adds a runtime service locator.
Both are extra dependencies for little benefit at this size (§31: dependencies must replace significant work).

## Decision

- No DI framework. The engine is a process-wide object (`SoundPush` in `core-engine`) started from the
  `Application`; platform callbacks are passed in explicitly (`PlatformDelegate`).
- Screens receive state and callbacks as parameters; feature modules depend only on `core-engine` and `core-ui`,
  never on each other.
- Revisit if the graph grows past roughly 15 modules or tests need many substituted collaborators.

## Consequences

- Faster builds and a smaller APK; wiring is visible in plain Kotlin.
- Tests substitute collaborators by constructor parameters or fakes rather than DI modules.
- Some care is needed to keep global state (the engine singleton) out of pure UI code.
