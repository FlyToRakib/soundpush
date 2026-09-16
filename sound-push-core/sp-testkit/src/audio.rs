//! Generated signals, a fake audio backend that plays one back, and the measurement helpers that
//! recover the delay between the two (plan §29.1 "Audio quality").

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sp_engine::sp_audio_io::{
    AudioBackend, AudioError, AudioStream, CaptureCallback, CaptureSource, DeviceInfo, DeviceKind,
    ErrorCallback, RenderCallback, RenderTarget, StreamInfo,
};

/// The wire sample rate, and the rate every fake device runs at.
pub const RATE: usize = 48_000;
/// Samples per device buffer (10 ms), matching the null backend.
const BUFFER: usize = RATE / 100;

/// One period of a probe signal: a linear chirp, then silence to the end of the period.
///
/// A chirp correlates sharply with itself, so the delay survives loss, concealment and resampling
/// far better than a tone does, and the silence keeps neighbouring periods from correlating with
/// each other. Use this period as the correlation reference; [`with_tone_floor`] turns it into the
/// signal to send.
pub fn chirp_period(period: usize, length: usize, from_hz: f32, to_hz: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; period.max(length)];
    let n = length.max(1) as f32;
    for (i, sample) in out.iter_mut().take(length).enumerate() {
        let t = i as f32 / RATE as f32;
        // Linear sweep: instantaneous frequency from_hz → to_hz over the burst.
        let rate = (to_hz - from_hz) / (n / RATE as f32);
        let phase = std::f32::consts::TAU * (from_hz * t + 0.5 * rate * t * t);
        // Raised-cosine edges keep the burst from clicking, which would smear the correlation peak.
        let edge = (RATE / 500).max(1); // 2 ms
        let window = match i {
            i if i < edge => 0.5 * (1.0 - (std::f32::consts::PI * i as f32 / edge as f32).cos()),
            i if i + edge >= length => {
                0.5 * (1.0 - (std::f32::consts::PI * (length - i) as f32 / edge as f32).cos())
            }
            _ => 1.0,
        };
        *sample = phase.sin() * 0.5 * window;
    }
    out
}

/// Replace a period's silence with a quiet tone.
///
/// A probe signal that is silent between bursts puts the encoder into DTX and lets the jitter
/// buffer drift, which is realistic but makes a latency reading wander. A tone well below the burst
/// keeps the stream continuous without disturbing the correlation, which only looks at the loud
/// part of the reference.
pub fn with_tone_floor(mut period: Vec<f32>, hz: f32, level: f32) -> Vec<f32> {
    for (i, sample) in period.iter_mut().enumerate() {
        if sample.abs() < level {
            let t = i as f32 / RATE as f32;
            *sample = (std::f32::consts::TAU * hz * t).sin() * level;
        }
    }
    period
}

/// Interleaved frames averaged down to one channel.
pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    let ch = channels.max(1);
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Largest absolute sample.
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0f32, |m, s| m.max(s.abs()))
}

/// Root mean square of the samples (0.0 for an empty slice).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// The shift `s` for which `window[i + s] == reference[i]`, in samples (0 ..< `reference.len()`).
///
/// The reference is one period of a repeating signal, so this is a circular correlation: the shift
/// with the largest match is the one that lines the bursts up. `None` when the window is shorter
/// than a period or nothing correlates (silence).
///
/// A full search over a one-second period is 48 000 shifts; instead the burst is located first by
/// correlating block energies (a hundred-odd values), and only the samples around that are
/// correlated properly. The answer is the same and the work is two orders of magnitude smaller.
pub fn correlation_shift(window: &[f32], reference: &[f32]) -> Option<usize> {
    let period = reference.len();
    if period == 0 || window.len() < period {
        return None;
    }
    let block = (period / 128).max(1);
    let coarse = block * coarse_shift(window, reference, block)?;
    // The coarse pass is accurate to one block; search two either side.
    let from = coarse + period - 2 * block.min(period / 2);
    let mut best = (0usize, 0f32);
    for step in 0..=4 * block {
        let shift = (from + step) % period;
        let mut sum = 0f32;
        // The reference is mostly silence, so only its loud samples are worth multiplying.
        for (i, r) in reference.iter().enumerate() {
            if r.abs() > 0.01 {
                sum += window[(i + shift) % period] * r;
            }
        }
        if sum > best.1 {
            best = (shift, sum);
        }
    }
    (best.1 > 0.0).then_some(best.0)
}

/// The same correlation over block energies, which finds the burst to within one block.
fn coarse_shift(window: &[f32], reference: &[f32], block: usize) -> Option<usize> {
    let blocks = reference.len() / block;
    let energy = |signal: &[f32]| -> Vec<f32> {
        (0..blocks)
            .map(|b| {
                signal[b * block..(b + 1) * block]
                    .iter()
                    .map(|s| s * s)
                    .sum()
            })
            .collect()
    };
    let (we, re) = (energy(window), energy(reference));
    let mut best = (0usize, 0f32);
    for shift in 0..blocks {
        let sum: f32 = re
            .iter()
            .enumerate()
            .map(|(i, r)| we[(i + shift) % blocks] * r)
            .sum();
        if sum > best.1 {
            best = (shift, sum);
        }
    }
    (best.1 > 0.0).then_some(best.0)
}

/// When a fake stream produced its first buffer, so recorded audio can be placed on a clock.
#[derive(Default)]
pub struct Marks {
    capture_started: Mutex<Option<Instant>>,
    render_started: Mutex<Option<Instant>>,
}

impl Marks {
    /// When the capture stream handed over its first buffer.
    pub fn capture_started(&self) -> Option<Instant> {
        *self.capture_started.lock().unwrap()
    }

    /// When the render stream asked for its first buffer.
    pub fn render_started(&self) -> Option<Instant> {
        *self.render_started.lock().unwrap()
    }

    fn mark(slot: &Mutex<Option<Instant>>) {
        let mut at = slot.lock().unwrap();
        if at.is_none() {
            *at = Some(Instant::now());
        }
    }
}

/// A fake backend whose capture loops a generated signal and whose render is recorded.
///
/// Like the null backend it ticks on a normal thread at real time, so a scenario runs for as long
/// as the audio it carries; unlike it, the signal is the caller's, which is what makes a
/// cross-correlation measurement possible.
#[derive(Clone)]
pub struct SignalBackend {
    /// Mono signal, looped by the capture stream.
    pub signal: Arc<Vec<f32>>,
    /// Rendered audio, appended interleaved.
    pub recorded: Arc<Mutex<Vec<f32>>>,
    /// First-buffer timestamps of both streams.
    pub marks: Arc<Marks>,
}

impl SignalBackend {
    pub fn new(signal: Vec<f32>) -> Self {
        Self {
            signal: Arc::new(signal),
            recorded: Arc::new(Mutex::new(Vec::new())),
            marks: Arc::new(Marks::default()),
        }
    }
}

struct SignalStream {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    info: StreamInfo,
}

impl AudioStream for SignalStream {
    fn info(&self) -> StreamInfo {
        self.info
    }
}

impl Drop for SignalStream {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn spawn_ticker(mut tick: impl FnMut(u64) + Send + 'static) -> (Arc<AtomicBool>, JoinHandle<()>) {
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    let handle = std::thread::spawn(move || {
        let start = Instant::now();
        let mut n = 0u64;
        while flag.load(Ordering::Relaxed) {
            tick(n);
            n += 1;
            let next = start + Duration::from_millis(10 * n);
            if let Some(wait) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
    });
    (running, handle)
}

fn stream_info(channels: u16) -> StreamInfo {
    StreamInfo {
        device_sample_rate: RATE as u32,
        device_channels: channels,
        channels,
        latency_ms: 10,
    }
}

impl AudioBackend for SignalBackend {
    fn name(&self) -> &'static str {
        "signal"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(vec![
            DeviceInfo {
                id: "signal-in".into(),
                name: "Signal input".into(),
                kind: DeviceKind::Input,
                is_default: true,
                channels: 2,
                sample_rate: RATE as u32,
            },
            DeviceInfo {
                id: "signal-out".into(),
                name: "Signal output".into(),
                kind: DeviceKind::Output,
                is_default: true,
                channels: 2,
                sample_rate: RATE as u32,
            },
        ])
    }

    fn supports_loopback(&self) -> bool {
        true
    }

    fn open_capture(
        &self,
        _source: &CaptureSource,
        channels: u16,
        mut on_audio: CaptureCallback,
        _on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let ch = channels.clamp(1, 2) as usize;
        let signal = self.signal.clone();
        let marks = self.marks.clone();
        let mut buf = vec![0.0f32; BUFFER * ch];
        let (running, thread) = spawn_ticker(move |n| {
            Marks::mark(&marks.capture_started);
            for (i, frame) in buf.chunks_exact_mut(ch).enumerate() {
                let at = (n as usize * BUFFER + i) % signal.len().max(1);
                frame.fill(signal.get(at).copied().unwrap_or(0.0));
            }
            on_audio(&buf);
        });
        Ok(Box::new(SignalStream {
            running,
            thread: Some(thread),
            info: stream_info(channels),
        }))
    }

    fn open_render(
        &self,
        _target: &RenderTarget,
        channels: u16,
        mut on_audio: RenderCallback,
        _on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let ch = channels.clamp(1, 2) as usize;
        let recorded = self.recorded.clone();
        let marks = self.marks.clone();
        let mut buf = vec![0.0f32; BUFFER * ch];
        let (running, thread) = spawn_ticker(move |_| {
            Marks::mark(&marks.render_started);
            on_audio(&mut buf);
            recorded.lock().unwrap().extend_from_slice(&buf);
        });
        Ok(Box::new(SignalStream {
            running,
            thread: Some(thread),
            info: stream_info(channels),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chirp_period_is_a_burst_followed_by_silence() {
        let period = chirp_period(RATE, RATE / 50, 500.0, 4_000.0);
        assert_eq!(period.len(), RATE);
        assert!(peak(&period[..RATE / 50]) > 0.4, "the burst is loud");
        assert_eq!(peak(&period[RATE / 50..]), 0.0, "the rest is silence");
    }

    #[test]
    fn correlation_finds_a_known_shift() {
        let period = chirp_period(4_800, 960, 500.0, 4_000.0);
        for shift in [0usize, 137, 2_400, 4_799] {
            let window: Vec<f32> = (0..period.len())
                .map(|i| period[(i + period.len() - shift) % period.len()])
                .collect();
            assert_eq!(correlation_shift(&window, &period), Some(shift), "{shift}");
        }
    }

    #[test]
    fn silence_correlates_with_nothing() {
        let period = chirp_period(4_800, 960, 500.0, 4_000.0);
        assert_eq!(correlation_shift(&vec![0.0; 4_800], &period), None);
        assert_eq!(correlation_shift(&[], &period), None);
    }

    #[test]
    fn a_tone_floor_fills_the_silence_and_leaves_the_burst_alone() {
        let chirp = chirp_period(RATE, RATE / 50, 500.0, 4_000.0);
        let probe = with_tone_floor(chirp.clone(), 220.0, 0.02);
        assert_eq!(probe.len(), chirp.len());
        assert!(
            peak(&probe[RATE / 25..]) > 0.0,
            "the silence now carries a tone"
        );
        assert!(peak(&probe[RATE / 25..]) <= 0.02, "and stays quiet");
        // The loud middle of the burst is untouched.
        let middle = RATE / 100;
        assert_eq!(probe[middle], chirp[middle]);
    }

    #[test]
    fn mono_averages_the_channels() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5], 2), [0.5, 0.5]);
        assert_eq!(to_mono(&[1.0, -1.0], 1), [1.0, -1.0]);
    }

    #[test]
    fn levels_are_measured_over_the_whole_slice() {
        assert_eq!(peak(&[0.1, -0.7, 0.3]), 0.7);
        assert_eq!(rms(&[1.0, -1.0]), 1.0);
        assert_eq!(rms(&[]), 0.0);
    }
}
