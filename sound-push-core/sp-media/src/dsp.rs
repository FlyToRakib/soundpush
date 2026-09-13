//! Small allocation-free DSP blocks used on real-time threads.

/// Convert decibels to linear gain.
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Convert linear amplitude to dBFS (floor −120 dB).
pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 1e-6 { -120.0 } else { 20.0 * gain.log10() }
}

/// Smoothed gain stage (avoids zipper noise on changes).
pub struct Gain {
    current: f32,
    target: f32,
}

impl Gain {
    pub fn new(gain: f32) -> Self {
        Self { current: gain, target: gain }
    }

    pub fn set(&mut self, gain: f32) {
        self.target = gain.clamp(0.0, 16.0);
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if (self.current - self.target).abs() < 1e-5 {
            if (self.current - 1.0).abs() > 1e-6 {
                samples.iter_mut().for_each(|s| *s *= self.current);
            }
            return;
        }
        let step = (self.target - self.current) / samples.len().max(1) as f32;
        for s in samples.iter_mut() {
            self.current += step;
            *s *= self.current;
        }
        self.current = self.target;
    }
}

/// Soft limiter: transparent below the threshold, smooth tanh knee above it.
pub struct SoftLimiter {
    threshold: f32,
}

impl Default for SoftLimiter {
    fn default() -> Self {
        Self { threshold: 0.89 } // ≈ −1 dBFS
    }
}

impl SoftLimiter {
    pub fn process(&self, samples: &mut [f32]) -> bool {
        let t = self.threshold;
        let headroom = 1.0 - t;
        let mut clipped = false;
        for s in samples.iter_mut() {
            let a = s.abs();
            if a > t {
                clipped = true;
                let over = (a - t) / headroom;
                *s = s.signum() * (t + headroom * over.tanh());
            }
        }
        clipped
    }
}

/// Peak/RMS level meter with decay, for UI level bars.
#[derive(Default)]
pub struct LevelMeter {
    peak: f32,
    rms: f32,
}

impl LevelMeter {
    pub fn process(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let mut peak = 0f32;
        let mut sum = 0f32;
        for s in samples {
            peak = peak.max(s.abs());
            sum += s * s;
        }
        let rms = (sum / samples.len() as f32).sqrt();
        self.peak = if peak > self.peak { peak } else { self.peak * 0.9 + peak * 0.1 };
        self.rms = self.rms * 0.8 + rms * 0.2;
    }

    pub fn peak_db(&self) -> f32 {
        gain_to_db(self.peak)
    }

    pub fn rms_db(&self) -> f32 {
        gain_to_db(self.rms)
    }
}

/// Mix interleaved `in_channels` audio down to `out_channels` (1 or 2).
/// Channels beyond front L/R are folded in at −3 dB.
pub fn downmix(input: &[f32], in_channels: usize, out_channels: usize, output: &mut Vec<f32>) {
    let in_ch = in_channels.max(1);
    let frames = input.len() / in_ch;
    output.reserve(frames * out_channels);
    const MINUS_3DB: f32 = 0.707;
    for f in 0..frames {
        let frame = &input[f * in_ch..(f + 1) * in_ch];
        let (l, r) = match in_ch {
            1 => (frame[0], frame[0]),
            2 => (frame[0], frame[1]),
            _ => {
                let extra: f32 = frame[2..].iter().sum::<f32>() * MINUS_3DB / (in_ch - 2) as f32;
                (frame[0] * MINUS_3DB + extra, frame[1] * MINUS_3DB + extra)
            }
        };
        if out_channels == 1 {
            output.push((l + r) * 0.5);
        } else {
            output.push(l);
            output.push(r);
        }
    }
}

/// Stereo balance (−1.0 = left only, 1.0 = right only) and optional mono.
pub fn balance_and_mono(stereo: &mut [f32], balance: f32, mono: bool) {
    let b = balance.clamp(-1.0, 1.0);
    let lg = if b > 0.0 { 1.0 - b } else { 1.0 };
    let rg = if b < 0.0 { 1.0 + b } else { 1.0 };
    for frame in stereo.chunks_exact_mut(2) {
        if mono {
            let m = (frame[0] + frame[1]) * 0.5;
            frame[0] = m;
            frame[1] = m;
        }
        frame[0] *= lg;
        frame[1] *= rg;
    }
}

pub fn s16_to_f32(input: &[i16], output: &mut Vec<f32>) {
    output.extend(input.iter().map(|&s| s as f32 / 32768.0));
}

pub fn f32_to_s16(input: &[f32], output: &mut Vec<i16>) {
    output.extend(input.iter().map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16));
}

/// True when every sample is below −60 dBFS (used for DTX).
pub fn is_silent(samples: &[f32]) -> bool {
    samples.iter().all(|s| s.abs() < 0.001)
}

/// RNNoise-based noise suppression (48 kHz mono frames of 480 samples).
pub struct NoiseSuppressor {
    state: Box<nnnoiseless::DenoiseState<'static>>,
    scratch_in: [f32; nnnoiseless::FRAME_SIZE],
    scratch_out: [f32; nnnoiseless::FRAME_SIZE],
}

impl Default for NoiseSuppressor {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseSuppressor {
    pub fn new() -> Self {
        Self {
            state: nnnoiseless::DenoiseState::new(),
            scratch_in: [0.0; nnnoiseless::FRAME_SIZE],
            scratch_out: [0.0; nnnoiseless::FRAME_SIZE],
        }
    }

    /// Process a mono buffer in place. Length must be a multiple of 480.
    pub fn process(&mut self, mono: &mut [f32]) {
        for chunk in mono.chunks_exact_mut(nnnoiseless::FRAME_SIZE) {
            // nnnoiseless expects i16-range floats.
            for (dst, src) in self.scratch_in.iter_mut().zip(chunk.iter()) {
                *dst = *src * 32768.0;
            }
            self.state.process_frame(&mut self.scratch_out, &self.scratch_in);
            for (dst, src) in chunk.iter_mut().zip(self.scratch_out.iter()) {
                *dst = *src / 32768.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_conversions() {
        assert!((db_to_gain(20.0) - 10.0).abs() < 1e-4);
        assert!((gain_to_db(1.0)).abs() < 1e-4);
    }

    #[test]
    fn limiter_never_exceeds_full_scale() {
        let mut s = vec![0.5, 1.5, -3.0, 0.95];
        assert!(SoftLimiter::default().process(&mut s));
        assert!(s.iter().all(|x| x.abs() <= 1.0));
        assert_eq!(s[0], 0.5);
    }

    #[test]
    fn downmix_51_to_stereo_and_mono() {
        let frame = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut out = Vec::new();
        downmix(&frame, 6, 2, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out[0] > out[1]);
        let mut mono = Vec::new();
        downmix(&[0.2, 0.4], 2, 1, &mut mono);
        assert!((mono[0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn balance_and_mono_work() {
        let mut s = vec![1.0, 0.0];
        balance_and_mono(&mut s, 0.0, true);
        assert_eq!(s, vec![0.5, 0.5]);
        let mut s = vec![1.0, 1.0];
        balance_and_mono(&mut s, 1.0, false);
        assert_eq!(s, vec![0.0, 1.0]);
    }

    #[test]
    fn gain_ramps_to_target() {
        let mut g = Gain::new(1.0);
        g.set(2.0);
        let mut s = vec![1.0; 100];
        g.process(&mut s);
        assert!(s[99] > 1.9);
        let mut s = vec![1.0; 10];
        g.process(&mut s);
        assert!(s.iter().all(|x| (x - 2.0).abs() < 1e-5));
    }

    #[test]
    fn noise_suppressor_runs() {
        let mut ns = NoiseSuppressor::new();
        let mut buf: Vec<f32> = (0..4800).map(|i| ((i * 7919) % 200) as f32 / 2000.0 - 0.05).collect();
        ns.process(&mut buf);
        assert!(buf.iter().all(|s| s.is_finite()));
    }
}
