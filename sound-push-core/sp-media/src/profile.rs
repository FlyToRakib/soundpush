//! User-facing latency and quality profiles mapped to concrete stream parameters.

use sp_protocol::Codec;
use sp_protocol::control::StreamProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyProfile {
    LowLatency,
    Balanced,
    Stable,
    Custom { min_ms: u32, max_ms: u32 },
}

impl LatencyProfile {
    /// (jitter min ms, jitter max ms, frame duration µs)
    pub fn params(self) -> (u32, u32, u32) {
        match self {
            Self::LowLatency => (10, 40, 5_000),
            Self::Balanced => (20, 80, 10_000),
            Self::Stable => (60, 250, 20_000),
            Self::Custom { min_ms, max_ms } => {
                let min = min_ms.clamp(5, 500);
                (min, max_ms.clamp(min, 1000), 10_000)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Opus with adaptive bitrate starting at 128 kb/s.
    Auto,
    /// Fixed Opus bitrate in bits/s (6 000 – 510 000).
    Opus(u32),
    /// Uncompressed 16-bit PCM.
    Lossless,
}

/// Bitrate presets shown in the UI, including every step AudioRelay offers.
pub const OPUS_BITRATE_STEPS: &[u32] = &[
    10_000, 16_000, 24_000, 32_000, 48_000, 64_000, 96_000, 128_000, 192_000, 256_000, 320_000,
    450_000, 510_000,
];

pub fn build_profile(
    latency: LatencyProfile,
    quality: Quality,
    channels: u32,
    redundancy: bool,
) -> StreamProfile {
    let (jitter_min_ms, jitter_max_ms, frame_us) = latency.params();
    let (codec, bitrate, adaptive) = match quality {
        Quality::Auto => (Codec::Opus, 128_000, true),
        Quality::Opus(bps) => (Codec::Opus, bps.clamp(6_000, 510_000), false),
        Quality::Lossless => (Codec::PcmS16Le, 0, false),
    };
    // PCM frames must fit in one datagram: 5 ms stereo or 10 ms mono = 960 bytes.
    let frame_us = if codec == Codec::PcmS16Le {
        frame_us.min(if channels > 1 { 5_000 } else { 10_000 })
    } else {
        frame_us
    };
    StreamProfile {
        codec: codec as u32,
        bitrate,
        channels: channels.clamp(1, 2),
        frame_us,
        jitter_min_ms,
        jitter_max_ms,
        redundancy,
        adaptive_bitrate: adaptive,
        // Where noise suppression runs is a microphone setting; the engine fills it in.
        denoise: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples_per_frame;
    use sp_protocol::media::MAX_MEDIA_PAYLOAD;

    #[test]
    fn pcm_frames_fit_in_datagram() {
        for lat in [
            LatencyProfile::LowLatency,
            LatencyProfile::Balanced,
            LatencyProfile::Stable,
        ] {
            for channels in [1, 2] {
                let p = build_profile(lat, Quality::Lossless, channels, false);
                let bytes = samples_per_frame(p.frame_us) * p.channels as usize * 2;
                assert!(
                    bytes <= MAX_MEDIA_PAYLOAD,
                    "{lat:?} {channels} ch -> {bytes}"
                );
            }
        }
    }

    #[test]
    fn custom_bounds_are_sane() {
        let (min, max, _) = LatencyProfile::Custom {
            min_ms: 1,
            max_ms: 0,
        }
        .params();
        assert_eq!(min, 5);
        assert!(max >= min);
    }
}
