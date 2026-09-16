//! Test and measurement helpers shared by the engine's simulation tests and the developer tools
//! (plan §12.1 `sp-testkit`, §29.1 "Simulation" and "Audio quality").
//!
//! Two pieces, usable together or apart:
//!
//! * [`net`] — a one-way datagram link with loss, jitter (and the reordering it causes),
//!   duplication and blackouts, plus the named impairment profiles `tools/netsim` applies.
//! * [`audio`] — generated signals (tone, chirp), a fake audio backend that plays one back and
//!   records what comes out, and cross-correlation to recover the delay between the two.
//!
//! [`Scenario`] joins them: a real sender and receiver pipeline with a simulated path in between.
//! Everything outside the transport boundary is the production pipeline — capture, DSP, Opus,
//! redundancy, jitter buffer, concealment, drift compensation and render.
//!
//! Impairments are drawn from a seeded RNG, so a failure reproduces.
// Test infrastructure: a poisoned lock or a pipeline that cannot start means the test or tool is
// already broken, so these fail loudly instead of degrading quietly.
#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod audio;
pub mod net;
mod scenario;

pub use audio::SignalBackend;
pub use net::{Impairments, LinkStats, SimLink};
pub use scenario::{Scenario, Source};
