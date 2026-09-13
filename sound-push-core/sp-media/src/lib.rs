//! SoundPush media processing. No OS calls; everything is deterministic and testable.
//!
//! Wire format is always 48 kHz. Samples inside the pipeline are interleaved `f32`
//! in the range [-1.0, 1.0].

pub mod codec;
pub mod drift;
pub mod dsp;
pub mod jitter;
pub mod profile;

/// Sample rate used on the wire and inside the pipeline.
pub const SAMPLE_RATE: u32 = 48_000;

/// Number of samples per channel for a frame of `frame_us` microseconds.
pub const fn samples_per_frame(frame_us: u32) -> usize {
    (SAMPLE_RATE as u64 * frame_us as u64 / 1_000_000) as usize
}

/// Convert milliseconds to samples (per channel) at 48 kHz.
pub const fn ms_to_samples(ms: u32) -> u64 {
    SAMPLE_RATE as u64 * ms as u64 / 1000
}

/// Convert samples (per channel) to milliseconds at 48 kHz.
pub fn samples_to_ms(samples: u64) -> f64 {
    samples as f64 * 1000.0 / SAMPLE_RATE as f64
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MediaError {
    #[error("codec error: {0}")]
    Codec(String),
    #[error("invalid parameter: {0}")]
    InvalidParameter(&'static str),
    #[error("payload has wrong size: {got} bytes, expected {expected}")]
    WrongPayloadSize { got: usize, expected: usize },
}
