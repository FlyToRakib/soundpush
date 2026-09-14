# ADR-0013: Fully free — no ads, no paid tiers, no telemetry

- Status: accepted
- Date: 2026-09-14
- Plan: §2.2, §36.1, decision log ADR-0013

## Context

AudioRelay uses a freemium model: ads, time limits and premium features (multi-device, quality settings, RNNoise).
SoundPush is an open-source project; users trust it with their microphone.

## Decision

- Every feature is available to everyone. No subscriptions, license keys, ads, time limits or payment-gated flags.
- No analytics SDKs, no telemetry, no automatic crash upload, no hosted backend. Users share diagnostics manually
  (PRIVACY.md).
- Costs that exist (code-signing certificates, Apple Developer account, test devices) are covered by optional
  donations (GitHub Sponsors / Open Collective). Donations never unlock features.
- Update checks contact GitHub only, send no identifiers and can be turned off.

## Consequences

- No billing or entitlement code; smaller apps and fewer failure modes.
- The project cannot measure usage or crashes automatically; it relies on issues, discussions and shared
  diagnostics, so the in-app troubleshooter and diagnostics export must be good.
- Paid signing is postponed until funded; releases work without it (ADR-0019).
