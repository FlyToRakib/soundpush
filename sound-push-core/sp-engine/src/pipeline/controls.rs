//! Lock-free controls and statistics shared between the engine and audio threads.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

/// `f32` stored in an `AtomicU32`.
#[derive(Debug, Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// Level of what this device is playing on its own speakers, in dBFS, updated by every receiver
/// that renders there. Microphone captures read it to duck while the other side is talking, which
/// is the desktop's stand-in for acoustic echo cancellation (plan §15.7).
#[derive(Debug)]
pub struct EchoReference(AtomicF32);

impl Default for EchoReference {
    fn default() -> Self {
        Self(AtomicF32::new(-120.0))
    }
}

impl EchoReference {
    pub fn set_level_db(&self, db: f32) {
        self.0.set(db);
    }

    pub fn level_db(&self) -> f32 {
        self.0.get()
    }
}

#[derive(Debug)]
pub struct SenderControls {
    pub muted: AtomicBool,
    pub gain_db: AtomicF32,
    pub noise_suppression: AtomicBool,
    /// 80 Hz high-pass ahead of noise suppression (microphone groups).
    pub high_pass: AtomicBool,
    /// Lower this microphone while the device plays the other side on its speakers.
    pub echo_ducking: AtomicBool,
    /// Requested Opus bitrate (bits/s); applied by the encoder thread when it changes.
    pub bitrate: AtomicU32,
    pub expected_loss_pct: AtomicU32,
    /// Append the previous frame to each packet so single losses can be recovered.
    pub redundancy: AtomicBool,
    // statistics
    pub frames_sent: AtomicU64,
    pub send_errors: AtomicU64,
    pub capture_overruns: AtomicU64,
    pub level_db: AtomicF32,
    pub clipping: AtomicBool,
    pub bytes_sent: AtomicU64,
}

impl SenderControls {
    pub fn new(gain_db: f32, noise_suppression: bool, bitrate: u32) -> Self {
        Self {
            muted: AtomicBool::new(false),
            gain_db: AtomicF32::new(gain_db),
            noise_suppression: AtomicBool::new(noise_suppression),
            high_pass: AtomicBool::new(false),
            echo_ducking: AtomicBool::new(false),
            bitrate: AtomicU32::new(bitrate),
            expected_loss_pct: AtomicU32::new(0),
            redundancy: AtomicBool::new(false),
            frames_sent: AtomicU64::new(0),
            send_errors: AtomicU64::new(0),
            capture_overruns: AtomicU64::new(0),
            level_db: AtomicF32::new(-120.0),
            clipping: AtomicBool::new(false),
            bytes_sent: AtomicU64::new(0),
        }
    }
}

#[derive(Debug)]
pub struct ReceiverControls {
    pub muted: AtomicBool,
    pub volume: AtomicF32,
    pub balance: AtomicF32,
    pub mono: AtomicBool,
    pub av_offset_ms: AtomicI32,
    /// Suppress noise in this incoming microphone stream, because its own device asked us to
    /// ("Noise suppression → on the other device", plan §15.7). Mono streams only.
    pub noise_suppression: AtomicBool,
    /// Jitter-buffer bounds (live profile changes).
    pub jitter_min_ms: AtomicU32,
    pub jitter_max_ms: AtomicU32,
    // statistics
    pub packets_received: AtomicU64,
    pub packets_missing: AtomicU64,
    pub packets_late: AtomicU64,
    /// Frames rebuilt from redundant copies.
    pub packets_recovered: AtomicU64,
    pub decode_errors: AtomicU64,
    pub underruns: AtomicU64,
    pub buffer_ms: AtomicF32,
    pub target_ms: AtomicF32,
    pub jitter_ms: AtomicF32,
    pub drift_ppm: AtomicI32,
    pub level_db: AtomicF32,
    pub device_latency_ms: AtomicU32,
    pub bytes_received: AtomicU64,
}

impl ReceiverControls {
    pub fn new(volume: f32, jitter_min_ms: u32, jitter_max_ms: u32) -> Self {
        Self {
            muted: AtomicBool::new(false),
            volume: AtomicF32::new(volume),
            balance: AtomicF32::new(0.0),
            mono: AtomicBool::new(false),
            av_offset_ms: AtomicI32::new(0),
            noise_suppression: AtomicBool::new(false),
            jitter_min_ms: AtomicU32::new(jitter_min_ms),
            jitter_max_ms: AtomicU32::new(jitter_max_ms),
            packets_received: AtomicU64::new(0),
            packets_missing: AtomicU64::new(0),
            packets_late: AtomicU64::new(0),
            packets_recovered: AtomicU64::new(0),
            decode_errors: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            buffer_ms: AtomicF32::new(0.0),
            target_ms: AtomicF32::new(0.0),
            jitter_ms: AtomicF32::new(0.0),
            drift_ppm: AtomicI32::new(0),
            level_db: AtomicF32::new(-120.0),
            device_latency_ms: AtomicU32::new(0),
            bytes_received: AtomicU64::new(0),
        }
    }
}
