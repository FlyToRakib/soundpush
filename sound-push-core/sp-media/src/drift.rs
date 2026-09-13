//! Clock drift compensation.
//!
//! The sender's and receiver's audio clocks run at slightly different rates.
//! [`DriftController`] is a PI controller on the jitter-buffer fill level that
//! outputs a playback ratio close to 1.0. [`FractionalResampler`] applies that
//! ratio with cubic Hermite interpolation, which is inaudible for the tiny
//! ratios involved (well under ±0.5 %).

/// Maximum correction applied (±0.5 %, about ±5000 ppm).
const MAX_CORRECTION: f64 = 0.005;

pub struct DriftController {
    kp: f64,
    ki: f64,
    integral: f64,
    filtered_error: Option<f64>,
    ratio: f64,
}

impl Default for DriftController {
    fn default() -> Self {
        Self::new()
    }
}

impl DriftController {
    pub fn new() -> Self {
        Self {
            kp: 2e-6,
            ki: 2e-9,
            integral: 0.0,
            filtered_error: None,
            ratio: 1.0,
        }
    }

    /// Update once per output frame.
    ///
    /// `buffered` and `target` are in samples. Returns the ratio of input samples
    /// to consume per output sample (> 1.0 means play slightly faster).
    pub fn update(&mut self, buffered: u64, target: u64) -> f64 {
        let error = buffered as f64 - target as f64;
        let filtered = match self.filtered_error {
            Some(prev) => prev + (error - prev) * 0.02,
            None => error,
        };
        self.filtered_error = Some(filtered);
        self.integral = (self.integral + filtered).clamp(-2e6, 2e6);
        let correction = (self.kp * filtered + self.ki * self.integral).clamp(-MAX_CORRECTION, MAX_CORRECTION);
        self.ratio = 1.0 + correction;
        self.ratio
    }

    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Current correction in parts per million.
    pub fn ppm(&self) -> i32 {
        ((self.ratio - 1.0) * 1e6).round() as i32
    }

    pub fn reset(&mut self) {
        self.integral = 0.0;
        self.filtered_error = None;
        self.ratio = 1.0;
    }
}

/// Streaming variable-ratio resampler for interleaved audio.
pub struct FractionalResampler {
    channels: usize,
    /// Last 4 input frames (per channel) for interpolation.
    history: Vec<[f32; 4]>,
    /// Fractional read position between history[1] and history[2].
    phase: f64,
    primed: usize,
}

impl FractionalResampler {
    pub fn new(channels: usize) -> Self {
        Self {
            channels: channels.max(1),
            history: vec![[0.0; 4]; channels.max(1)],
            phase: 0.0,
            primed: 0,
        }
    }

    /// Resample `input` with `ratio` (input samples consumed per output sample),
    /// appending to `output`. Consumes all of `input`.
    pub fn process(&mut self, input: &[f32], ratio: f64, output: &mut Vec<f32>) {
        let ch = self.channels;
        let frames = input.len() / ch;
        for f in 0..frames {
            for c in 0..ch {
                let h = &mut self.history[c];
                h.rotate_left(1);
                h[3] = input[f * ch + c];
            }
            self.primed = (self.primed + 1).min(4);
            if self.primed < 4 {
                continue;
            }
            // Emit output samples while the read position lies within [h1, h2).
            while self.phase < 1.0 {
                let t = self.phase as f32;
                for c in 0..ch {
                    output.push(hermite(&self.history[c], t));
                }
                self.phase += ratio;
            }
            self.phase -= 1.0;
        }
    }

    pub fn reset(&mut self) {
        self.history.fill([0.0; 4]);
        self.phase = 0.0;
        self.primed = 0;
    }
}

#[inline]
fn hermite(h: &[f32; 4], t: f32) -> f32 {
    let (y0, y1, y2, y3) = (h[0], h[1], h[2], h[3]);
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + c0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_ratio_preserves_length_and_signal() {
        let mut r = FractionalResampler::new(1);
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut out = Vec::new();
        r.process(&input, 1.0, &mut out);
        assert!((out.len() as i64 - 4797).abs() <= 1, "len {}", out.len());
        // Output is delayed by one sample relative to input.
        for (i, s) in out.iter().enumerate().skip(10).take(100) {
            assert!((s - input[i + 1]).abs() < 1e-3);
        }
    }

    #[test]
    fn ratio_changes_output_length() {
        let mut r = FractionalResampler::new(2);
        let input = vec![0.25f32; 48_000 * 2];
        let mut out = Vec::new();
        r.process(&input, 1.001, &mut out);
        let frames = out.len() / 2;
        let expected = 48_000.0 / 1.001;
        assert!((frames as f64 - expected).abs() < 5.0, "frames {frames}");
    }

    #[test]
    fn controller_converges_fill_under_clock_drift() {
        // Sender clock is 200 ppm fast: 480.096 samples arrive per 480 played.
        let target = 2_400u64;
        let mut buffered = target as f64;
        let mut ctrl = DriftController::new();
        let mut max_err: f64 = 0.0;
        for step in 0..360_000 {
            // one hour of 10 ms frames
            let ratio = ctrl.update(buffered as u64, target);
            buffered += 480.096 - 480.0 * ratio;
            if step > 60_000 {
                max_err = max_err.max((buffered - target as f64).abs());
            }
        }
        assert!(max_err < 480.0, "fill drifted by {max_err} samples");
        assert!((ctrl.ppm() - 200).abs() < 40, "ppm {}", ctrl.ppm());
    }
}
