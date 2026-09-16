//! Real-time audio pipelines.
//!
//! - [`sender`]: capture → DSP → encode → datagrams (encoder thread fed by a lock-free ring).
//! - [`receiver`]: datagrams → jitter buffer → decode → drift compensation → render.
//! - [`monitor`]: local mic monitoring (capture → render on this device).

pub mod controls;
pub mod monitor;
pub mod receiver;
pub mod sender;

pub use controls::{EchoReference, ReceiverControls, SenderControls};
