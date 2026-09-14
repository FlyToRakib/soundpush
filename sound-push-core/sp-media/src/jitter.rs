//! Adaptive jitter buffer.
//!
//! Holds decoded frames ordered by sender sample timestamp and plays them out at
//! the receiver's render pace. The target delay adapts to measured network jitter
//! within `[min_delay, max_delay]`: it grows immediately after an underrun and
//! shrinks slowly (the drift compensator consumes the extra samples inaudibly).

use std::collections::BTreeMap;

use crate::{ms_to_samples, samples_to_ms};

#[derive(Debug, Clone, Copy)]
pub struct JitterConfig {
    pub channels: usize,
    /// Samples per channel in one frame.
    pub frame_samples: usize,
    pub min_delay_ms: u32,
    pub max_delay_ms: u32,
}

/// What the buffer produced for one output frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopStatus {
    /// A received frame was played.
    Played,
    /// The frame was missing; the caller should run packet loss concealment.
    Missing,
    /// Not enough audio buffered yet; output is silence.
    Buffering,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct JitterStats {
    pub received: u64,
    pub played: u64,
    pub missing: u64,
    pub late: u64,
    pub underruns: u64,
    pub jitter_ms: f64,
    pub target_ms: f64,
    pub buffered_ms: f64,
}

pub struct JitterBuffer {
    cfg: JitterConfig,
    frames: BTreeMap<u64, Vec<f32>>,
    /// Sender timestamp of the next frame to play.
    play_ts: Option<u64>,
    /// Highest end timestamp received.
    newest_end_ts: u64,
    buffering: bool,
    consecutive_missing: u32,
    target_samples: u64,
    // Jitter estimation (RFC 3550 style, plus a percentile window).
    last_transit: Option<i64>,
    jitter_samples: f64,
    deviations: Vec<u32>,
    /// Preallocated scratch for the percentile (adapt runs on the audio thread).
    sorted: Vec<u32>,
    dev_cursor: usize,
    stats: JitterStats,
}

const WINDOW: usize = 512;
/// Frames of concealment before falling back to rebuffering.
const MAX_CONCEAL_FRAMES: u32 = 5;
/// Hard cap on buffered frames, whatever timestamps a sender invents. The largest legitimate
/// buffer (1.5 s custom delay plus A/V offset, 2.5 ms frames) needs about 700.
pub const MAX_FRAMES: usize = 1024;

impl JitterBuffer {
    pub fn new(cfg: JitterConfig) -> Self {
        let min = ms_to_samples(cfg.min_delay_ms);
        Self {
            cfg,
            frames: BTreeMap::new(),
            play_ts: None,
            newest_end_ts: 0,
            buffering: true,
            consecutive_missing: 0,
            target_samples: min.max(cfg.frame_samples as u64),
            last_transit: None,
            jitter_samples: 0.0,
            deviations: Vec::with_capacity(WINDOW),
            sorted: Vec::with_capacity(WINDOW),
            dev_cursor: 0,
            stats: JitterStats::default(),
        }
    }

    pub fn config(&self) -> JitterConfig {
        self.cfg
    }

    /// Change the delay bounds live (latency profile change).
    pub fn set_bounds(&mut self, min_delay_ms: u32, max_delay_ms: u32) {
        self.cfg.min_delay_ms = min_delay_ms;
        self.cfg.max_delay_ms = max_delay_ms.max(min_delay_ms);
        self.target_samples = self.clamp_target(self.target_samples);
    }

    /// Insert a decoded frame. `arrival_samples` is the receiver's local clock
    /// expressed in samples (e.g. monotonic microseconds * 48 / 1000).
    pub fn push(&mut self, sender_ts: u64, samples: Vec<f32>, arrival_samples: u64) {
        self.stats.received += 1;
        self.update_jitter(sender_ts, arrival_samples);

        if let Some(play) = self.play_ts {
            if sender_ts < play {
                self.stats.late += 1;
                return;
            }
        }
        let frame_len = (samples.len() / self.cfg.channels.max(1)) as u64;
        // Timestamps are untrusted: never let them overflow.
        self.newest_end_ts = self.newest_end_ts.max(sender_ts.saturating_add(frame_len));
        self.frames.insert(sender_ts, samples);
        while self.frames.len() > MAX_FRAMES {
            self.frames.pop_first();
            self.stats.late += 1;
        }

        // Far too much buffered (e.g. the receiver stalled): jump forward.
        let limit = ms_to_samples(self.cfg.max_delay_ms) + ms_to_samples(200);
        if let Some(play) = self.play_ts {
            if self.newest_end_ts.saturating_sub(play) > limit {
                let new_play = self.newest_end_ts.saturating_sub(self.target_samples);
                self.frames = self.frames.split_off(&new_play);
                self.play_ts = self.frames.keys().next().copied();
            }
        }
    }

    /// True when a frame at `ts` is already buffered or has already been played.
    pub fn has(&self, ts: u64) -> bool {
        self.frames.contains_key(&ts) || self.play_ts.is_some_and(|p| ts < p)
    }

    /// Samples currently buffered ahead of the play position.
    pub fn buffered_samples(&self) -> u64 {
        match self.play_ts {
            Some(play) => self.newest_end_ts.saturating_sub(play),
            None => self
                .frames
                .keys()
                .next()
                .map_or(0, |first| self.newest_end_ts.saturating_sub(*first)),
        }
    }

    pub fn target_samples(&self) -> u64 {
        self.target_samples
    }

    /// Produce the next frame into `out` (length = frame_samples * channels).
    pub fn pop(&mut self, out: &mut [f32]) -> PopStatus {
        if self.buffering {
            if self.buffered_samples() < self.target_samples || self.frames.is_empty() {
                out.fill(0.0);
                return PopStatus::Buffering;
            }
            self.buffering = false;
            if self.play_ts.is_none() {
                self.play_ts = self.frames.keys().next().copied();
            }
        }

        let Some(play) = self.play_ts else {
            out.fill(0.0);
            return PopStatus::Buffering;
        };

        // Drop anything older than the play position.
        while let Some((&ts, _)) = self.frames.first_key_value() {
            if ts < play {
                self.frames.pop_first();
                self.stats.late += 1;
            } else {
                break;
            }
        }

        let advance = self.cfg.frame_samples as u64;
        if let Some(frame) = self.frames.remove(&play) {
            let n = frame.len().min(out.len());
            out[..n].copy_from_slice(&frame[..n]);
            out[n..].fill(0.0);
            self.play_ts =
                Some(play.saturating_add((frame.len() / self.cfg.channels.max(1)) as u64));
            self.consecutive_missing = 0;
            self.stats.played += 1;
            return PopStatus::Played;
        }

        // Missing frame.
        self.stats.missing += 1;
        self.consecutive_missing += 1;
        let have_future = !self.frames.is_empty();

        // Buffer is below target: hold the play position (conceal without advancing).
        // This grows the effective delay by one frame so the late packet can still play.
        if self.buffered_samples() + advance <= self.target_samples
            && self.consecutive_missing <= MAX_CONCEAL_FRAMES
        {
            return PopStatus::Missing;
        }
        if !have_future && self.consecutive_missing > MAX_CONCEAL_FRAMES {
            // Genuine underrun: rebuffer with a larger target.
            self.stats.underruns += 1;
            self.target_samples =
                self.clamp_target(self.target_samples.saturating_add(2 * advance));
            self.buffering = true;
            self.play_ts = None;
            self.consecutive_missing = 0;
            out.fill(0.0);
            return PopStatus::Buffering;
        }
        self.play_ts = Some(play.saturating_add(advance));
        PopStatus::Missing
    }

    /// Slowly converge the target towards what the measured jitter needs.
    /// Call about once per second.
    pub fn adapt(&mut self) {
        let needed = (self.percentile_deviation(0.98) as u64)
            .saturating_mul(2)
            .saturating_add(self.cfg.frame_samples as u64);
        let needed = self.clamp_target(needed);
        if needed > self.target_samples {
            self.target_samples = needed;
        } else {
            // Shrink by at most 1 ms per call.
            let step = ms_to_samples(1);
            self.target_samples =
                self.clamp_target(self.target_samples.saturating_sub(step).max(needed));
        }
    }

    pub fn stats(&self) -> JitterStats {
        JitterStats {
            jitter_ms: samples_to_ms(self.jitter_samples as u64),
            target_ms: samples_to_ms(self.target_samples),
            buffered_ms: samples_to_ms(self.buffered_samples()),
            ..self.stats
        }
    }

    /// Clear everything (e.g. after a discontinuity).
    pub fn reset(&mut self) {
        self.frames.clear();
        self.play_ts = None;
        self.newest_end_ts = 0;
        self.buffering = true;
        self.consecutive_missing = 0;
        self.last_transit = None;
    }

    fn clamp_target(&self, samples: u64) -> u64 {
        let min = ms_to_samples(self.cfg.min_delay_ms).max(self.cfg.frame_samples as u64);
        let max = ms_to_samples(self.cfg.max_delay_ms).max(min);
        samples.clamp(min, max)
    }

    fn update_jitter(&mut self, sender_ts: u64, arrival: u64) {
        // Wrapping arithmetic: a hostile timestamp must not overflow (it only skews the estimate).
        let transit = (arrival as i64).wrapping_sub(sender_ts as i64);
        if let Some(last) = self.last_transit {
            let d = transit.wrapping_sub(last).unsigned_abs();
            self.jitter_samples += (d as f64 - self.jitter_samples) / 16.0;
            let d = d.min(u32::MAX as u64) as u32;
            if self.deviations.len() < WINDOW {
                self.deviations.push(d);
            } else {
                self.deviations[self.dev_cursor] = d;
                self.dev_cursor = (self.dev_cursor + 1) % WINDOW;
            }
        }
        self.last_transit = Some(transit);
    }

    fn percentile_deviation(&mut self, p: f64) -> u32 {
        if self.deviations.is_empty() {
            return 0;
        }
        self.sorted.clear();
        self.sorted.extend_from_slice(&self.deviations);
        self.sorted.sort_unstable();
        let idx = ((self.sorted.len() - 1) as f64 * p).round() as usize;
        self.sorted[idx.min(self.sorted.len() - 1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};

    const FRAME: usize = 480;

    fn cfg(min: u32, max: u32) -> JitterConfig {
        JitterConfig {
            channels: 1,
            frame_samples: FRAME,
            min_delay_ms: min,
            max_delay_ms: max,
        }
    }

    fn frame(ts: u64) -> Vec<f32> {
        vec![(ts / FRAME as u64) as f32; FRAME]
    }

    #[test]
    fn buffers_then_plays_in_order() {
        let mut jb = JitterBuffer::new(cfg(20, 80));
        let mut out = vec![0.0; FRAME];
        for i in 0..3u64 {
            jb.push(i * FRAME as u64, frame(i * FRAME as u64), i * FRAME as u64);
        }
        assert_eq!(jb.pop(&mut out), PopStatus::Played);
        assert_eq!(out[0], 0.0);
        assert_eq!(jb.pop(&mut out), PopStatus::Played);
        assert_eq!(out[0], 1.0);
    }

    #[test]
    fn reordered_packets_play_in_order_and_gaps_conceal() {
        let mut jb = JitterBuffer::new(cfg(30, 80));
        let mut out = vec![0.0; FRAME];
        for i in [1u64, 0, 3, 2, 5] {
            jb.push(i * FRAME as u64, frame(i * FRAME as u64), 0);
        }
        let mut seen = Vec::new();
        for _ in 0..5 {
            match jb.pop(&mut out) {
                PopStatus::Played => seen.push(out[0] as u64),
                PopStatus::Missing => seen.push(99),
                PopStatus::Buffering => {}
            }
        }
        assert_eq!(seen, vec![0, 1, 2, 3, 99]);
    }

    #[test]
    fn late_packets_are_dropped() {
        let mut jb = JitterBuffer::new(cfg(20, 80));
        let mut out = vec![0.0; FRAME];
        for i in 0..4u64 {
            jb.push(i * FRAME as u64, frame(i * FRAME as u64), 0);
        }
        jb.pop(&mut out);
        jb.pop(&mut out);
        jb.push(0, frame(0), 0);
        assert_eq!(jb.stats().late, 1);
    }

    #[test]
    fn hostile_timestamps_never_panic_and_stay_bounded() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        let mut jb = JitterBuffer::new(cfg(20, 1000));
        let mut out = vec![0.0; FRAME];
        for i in 0..20_000u64 {
            let ts = match i % 4 {
                0 => u64::MAX - rng.gen_range(0..1000),
                1 => rng.r#gen::<u64>(),
                2 => i * 7,
                _ => rng.gen_range(0..u64::MAX / 2),
            };
            let arrival = if i % 3 == 0 { u64::MAX } else { rng.r#gen() };
            jb.push(ts, frame(0), arrival);
            assert!(jb.frames.len() <= MAX_FRAMES);
            if i % 5 == 0 {
                jb.pop(&mut out);
            }
            if i % 97 == 0 {
                jb.adapt();
            }
        }
        let _ = jb.stats();
    }

    #[test]
    fn random_jitter_within_bounds_has_no_underruns_after_warmup() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let mut jb = JitterBuffer::new(cfg(20, 120));
        let mut out = vec![0.0; FRAME];
        // Packets sent every 10 ms, delayed by 0–30 ms; receiver pops every 10 ms.
        let mut pending: Vec<(u64, u64)> = Vec::new();
        let total = 3000u64;
        for tick in 0..total {
            let ts = tick * FRAME as u64;
            let delay = rng.gen_range(0..=(30 * 48));
            pending.push((ts + delay, ts));
            let now = ts;
            pending.sort_unstable();
            while let Some(&(arrive, sts)) = pending.first() {
                if arrive <= now {
                    jb.push(sts, frame(sts), arrive);
                    pending.remove(0);
                } else {
                    break;
                }
            }
            jb.pop(&mut out);
            if tick % 100 == 0 {
                jb.adapt();
            }
        }
        let stats = jb.stats();
        assert!(stats.underruns <= 3, "underruns {}", stats.underruns);
        assert!(
            stats.played as f64 > total as f64 * 0.95,
            "played {}",
            stats.played
        );
        assert!(stats.target_ms <= 120.0);
    }
}
