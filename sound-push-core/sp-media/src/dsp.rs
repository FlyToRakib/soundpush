//! Small allocation-free DSP blocks used on real-time threads.

/// Convert decibels to linear gain.
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Convert linear amplitude to dBFS (floor −120 dB).
pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 1e-6 {
        -120.0
    } else {
        20.0 * gain.log10()
    }
}

/// Smoothed gain stage (avoids zipper noise on changes).
pub struct Gain {
    current: f32,
    target: f32,
}

impl Gain {
    pub fn new(gain: f32) -> Self {
        Self {
            current: gain,
            target: gain,
        }
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

/// Second-order Butterworth high-pass filter (RBJ biquad) for interleaved 48 kHz audio.
/// The microphone chain uses it at 80 Hz, ahead of noise suppression (plan §15.7).
pub struct HighPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    /// Per channel: x[n-1], x[n-2], y[n-1], y[n-2].
    state: Vec<[f32; 4]>,
}

impl HighPass {
    pub fn new(cutoff_hz: f32, channels: usize) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz.clamp(1.0, 20_000.0) / 48_000.0;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 + cos) / 2.0 / a0,
            b1: -(1.0 + cos) / a0,
            b2: (1.0 + cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            state: vec![[0.0; 4]; channels.max(1)],
        }
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        let channels = self.state.len();
        for frame in samples.chunks_exact_mut(channels) {
            for (s, st) in frame.iter_mut().zip(self.state.iter_mut()) {
                let x = *s;
                let mut y = self.b0 * x + self.b1 * st[0] + self.b2 * st[1]
                    - self.a1 * st[2]
                    - self.a2 * st[3];
                // Decaying silence would otherwise reach denormals, which are slow on x86.
                if y.abs() < 1e-20 {
                    y = 0.0;
                }
                *st = [x, st[0], y, st[2]];
                *s = y;
            }
        }
    }

    pub fn reset(&mut self) {
        self.state.iter_mut().for_each(|st| *st = [0.0; 4]);
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
        self.peak = if peak > self.peak {
            peak
        } else {
            self.peak * 0.9 + peak * 0.1
        };
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
            self.state
                .process_frame(&mut self.scratch_out, &self.scratch_in);
            for (dst, src) in chunk.iter_mut().zip(self.scratch_out.iter()) {
                *dst = *src / 32768.0;
            }
        }
    }
}

/// Chunk of audio one feedback-analysis step looks at (10 ms at 48 kHz).
const FEEDBACK_CHUNK: usize = 480;
/// Shortest and longest autocorrelation lag examined: 4 kHz down to 200 Hz.
const FEEDBACK_MIN_LAG: usize = 12;
const FEEDBACK_MAX_LAG: usize = 240;
/// How long a howl has to persist before it is reported (0.8 s).
const FEEDBACK_CHUNKS: u32 = 80;

/// Detects an acoustic feedback loop ("howl") in a microphone that is played back on the same
/// device's speakers (plan §8.2).
///
/// A howl is a loud, near-sinusoidal tone that holds its pitch while its level builds up. Speech
/// is periodic too, but its pitch moves and voiced parts are broken by consonants and pauses, so
/// a run never lasts long enough. Fans, keyboards and room noise have no periodicity at all.
#[derive(Default)]
pub struct FeedbackDetector {
    /// Samples collected towards the next analysis chunk.
    buffer: Vec<f32>,
    /// Consecutive chunks that looked like a howl, and the level the run started at.
    run: u32,
    run_start_rms: f32,
    detected: bool,
}

impl FeedbackDetector {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(FEEDBACK_CHUNK * 2),
            ..Self::default()
        }
    }

    /// Feed mono audio. Returns true while a feedback loop is being heard.
    pub fn process(&mut self, mono: &[f32]) -> bool {
        // A caller feeding much more than one chunk at a time must not make the buffer grow.
        if self.buffer.len() > FEEDBACK_CHUNK {
            self.buffer.clear();
        }
        self.buffer.extend_from_slice(mono);
        while self.buffer.len() >= FEEDBACK_CHUNK {
            let chunk: Vec<f32> = self.buffer.drain(..FEEDBACK_CHUNK).collect();
            self.analyze(&chunk);
        }
        self.detected
    }

    /// Forget the current state (the monitor stopped, or the user has been told).
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.run = 0;
        self.run_start_rms = 0.0;
        self.detected = false;
    }

    fn analyze(&mut self, chunk: &[f32]) {
        let energy: f32 = chunk.iter().map(|s| s * s).sum();
        let rms = (energy / chunk.len() as f32).sqrt();
        // Quiet audio is never feedback, however periodic it looks.
        if rms < db_to_gain(-35.0) || periodicity(chunk) < 0.9 {
            self.run = 0;
            self.detected = false;
            return;
        }
        if self.run == 0 {
            self.run_start_rms = rms;
        }
        self.run += 1;
        // Either the level built up the way a loop does, or it is already howling loudly.
        let grew = rms > self.run_start_rms * 1.5 || rms > db_to_gain(-12.0);
        self.detected = self.run >= FEEDBACK_CHUNKS && grew;
    }
}

/// Highest normalized autocorrelation over the examined lags (1.0 = perfectly periodic).
fn periodicity(chunk: &[f32]) -> f32 {
    let window = chunk.len().saturating_sub(FEEDBACK_MAX_LAG);
    let power: f32 = chunk[..window].iter().map(|s| s * s).sum();
    if power <= f32::EPSILON {
        return 0.0;
    }
    let mut best = 0.0f32;
    for lag in FEEDBACK_MIN_LAG..=FEEDBACK_MAX_LAG {
        let mut num = 0.0f32;
        let mut den = 0.0f32;
        for (i, a) in chunk[..window].iter().enumerate() {
            let b = chunk[i + lag];
            num += a * b;
            den += b * b;
        }
        if den > f32::EPSILON {
            best = best.max(num / (power * den).sqrt());
        }
    }
    best
}

/// Far-end level (dBFS) above which the microphone is ducked.
const DUCK_THRESHOLD_DB: f32 = -45.0;
/// How far the microphone is lowered while the far side is heard.
const DUCK_DEPTH_DB: f32 = -18.0;
/// Blocks the duck is held after the far side falls quiet, so word gaps do not pump.
const DUCK_HOLD_BLOCKS: u32 = 20;

/// Half-duplex echo control: lowers the microphone while this device plays the far side on its
/// own speakers. Used where no acoustic echo canceller is available (plan §15.7 and
/// docs/adr/0020-desktop-echo-control.md).
pub struct DuckGate {
    gain: Gain,
    target: f32,
    hold: u32,
}

impl Default for DuckGate {
    fn default() -> Self {
        Self::new()
    }
}

impl DuckGate {
    pub fn new() -> Self {
        Self {
            gain: Gain::new(1.0),
            target: 1.0,
            hold: 0,
        }
    }

    /// Apply the duck to one block. `far_db` is what this device is playing right now.
    pub fn process(&mut self, samples: &mut [f32], far_db: f32) {
        if far_db > DUCK_THRESHOLD_DB {
            self.target = db_to_gain(DUCK_DEPTH_DB);
            self.hold = DUCK_HOLD_BLOCKS;
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.target = 1.0;
        }
        self.gain.set(self.target);
        self.gain.process(samples);
    }

    /// The gain the gate is heading for (1.0 = open). For tests and diagnostics.
    pub fn target(&self) -> f32 {
        self.target
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
    fn high_pass_removes_rumble_and_keeps_speech() {
        let tone = |freq: f32| -> f32 {
            let mut hp = HighPass::new(80.0, 2);
            let mut buf: Vec<f32> = (0..48_000 * 2)
                .map(|i| (2.0 * std::f32::consts::PI * freq * (i / 2) as f32 / 48_000.0).sin())
                .collect();
            hp.process(&mut buf);
            // Peak over the last half second, after the filter settled.
            buf[48_000..].iter().fold(0f32, |m, s| m.max(s.abs()))
        };
        assert!(tone(1000.0) > 0.97, "1 kHz passes");
        assert!(tone(20.0) < 0.1, "20 Hz is attenuated");
        let dc = {
            let mut hp = HighPass::new(80.0, 1);
            let mut buf = vec![0.5f32; 48_000];
            hp.process(&mut buf);
            buf[47_999].abs()
        };
        assert!(dc < 1e-3, "DC removed: {dc}");
    }

    #[test]
    fn noise_suppressor_runs() {
        let mut ns = NoiseSuppressor::new();
        let mut buf: Vec<f32> = (0..4800)
            .map(|i| ((i * 7919) % 200) as f32 / 2000.0 - 0.05)
            .collect();
        ns.process(&mut buf);
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    /// A rising tone the way a room builds up a howl.
    fn howl(seconds: f32) -> Vec<f32> {
        let n = (48_000.0 * seconds) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                let level = (0.02 * (t * 3.0).exp()).min(0.7);
                level * (2.0 * std::f32::consts::PI * 1_450.0 * t).sin()
            })
            .collect()
    }

    /// Loud speech-like audio: a harmonic stack whose pitch moves, with syllable gaps.
    fn speech(seconds: f32) -> Vec<f32> {
        let n = (48_000.0 * seconds) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                // Four syllables a second, with a gap between them.
                let syllable = (t * 4.0).fract();
                if syllable > 0.75 {
                    return 0.0;
                }
                let envelope = 0.6 * (syllable / 0.75 * std::f32::consts::PI).sin();
                let f0 = 140.0 + 60.0 * (2.0 * std::f32::consts::PI * 1.7 * t).sin();
                let phase = 2.0 * std::f32::consts::PI * f0 * t;
                envelope * (phase.sin() + 0.5 * (2.0 * phase).sin() + 0.3 * (3.0 * phase).sin())
                    / 1.8
            })
            .collect()
    }

    #[test]
    fn feedback_detector_finds_a_howl() {
        let mut d = FeedbackDetector::new();
        let audio = howl(2.0);
        let mut fired = false;
        for block in audio.chunks(240) {
            fired |= d.process(block);
        }
        assert!(fired, "a building 1.45 kHz tone is a feedback loop");
        d.reset();
        assert!(!d.process(&[0.0; 480]));
    }

    #[test]
    fn feedback_detector_ignores_speech_and_noise() {
        let mut d = FeedbackDetector::new();
        for block in speech(5.0).chunks(240) {
            assert!(
                !d.process(block),
                "loud speech must not warn about feedback"
            );
        }

        let mut d = FeedbackDetector::new();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let noise: Vec<f32> = (0..48_000 * 3)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 40) as f32 / 8_388_608.0 - 1.0
            })
            .collect();
        for block in noise.chunks(240) {
            assert!(!d.process(block), "loud noise must not warn about feedback");
        }
    }

    #[test]
    fn duck_gate_lowers_the_microphone_while_the_far_side_talks() {
        let mut gate = DuckGate::new();
        let mut buf = vec![1.0f32; 480];
        // Far side quiet: the microphone passes through.
        gate.process(&mut buf, -80.0);
        assert!((gate.target() - 1.0).abs() < 1e-6);
        assert!(buf.iter().all(|s| (*s - 1.0).abs() < 1e-5));

        // Far side talking: ducked, and held through a short gap.
        for _ in 0..4 {
            buf.fill(1.0);
            gate.process(&mut buf, -20.0);
        }
        assert!(gate.target() < 0.2);
        assert!(buf[479] < 0.2, "ducked sample {}", buf[479]);
        buf.fill(1.0);
        gate.process(&mut buf, -80.0);
        assert!(gate.target() < 0.2, "the duck is held over word gaps");

        // Far side quiet for longer: the microphone opens again.
        for _ in 0..40 {
            buf.fill(1.0);
            gate.process(&mut buf, -80.0);
        }
        assert!((gate.target() - 1.0).abs() < 1e-6);
        assert!(buf[479] > 0.9);
    }
}
