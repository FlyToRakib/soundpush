//! Device-format ↔ pipeline-format conversion, allocation-free after warm-up.

use sp_media::SAMPLE_RATE;
use sp_media::drift::FractionalResampler;
use sp_media::dsp::downmix;

/// Converts device capture buffers (any rate, any channel count) to 48 kHz frames
/// with the requested channel count.
pub struct CaptureConverter {
    device_rate: u32,
    device_channels: usize,
    out_channels: usize,
    resampler: Option<FractionalResampler>,
    mixed: Vec<f32>,
    resampled: Vec<f32>,
}

impl CaptureConverter {
    pub fn new(device_rate: u32, device_channels: u16, out_channels: u16) -> Self {
        let out_channels = out_channels.clamp(1, 2) as usize;
        Self {
            device_rate,
            device_channels: device_channels.max(1) as usize,
            out_channels,
            resampler: (device_rate != SAMPLE_RATE).then(|| FractionalResampler::new(out_channels)),
            mixed: Vec::with_capacity(16_384),
            resampled: Vec::with_capacity(16_384),
        }
    }

    /// Convert and return the 48 kHz buffer.
    pub fn process(&mut self, input: &[f32]) -> &[f32] {
        self.mixed.clear();
        if self.device_channels == self.out_channels {
            self.mixed.extend_from_slice(input);
        } else {
            downmix(
                input,
                self.device_channels,
                self.out_channels,
                &mut self.mixed,
            );
        }
        match self.resampler.as_mut() {
            None => &self.mixed,
            Some(r) => {
                self.resampled.clear();
                let ratio = self.device_rate as f64 / SAMPLE_RATE as f64;
                r.process(&self.mixed, ratio, &mut self.resampled);
                &self.resampled
            }
        }
    }
}

/// Pulls 48 kHz frames from a pipeline callback and produces device buffers.
pub struct RenderConverter {
    device_rate: u32,
    device_channels: usize,
    in_channels: usize,
    resampler: Option<FractionalResampler>,
    pipeline: Vec<f32>,
    resampled: Vec<f32>,
    /// Converted device-rate samples not yet delivered.
    pending: Vec<f32>,
    pending_pos: usize,
}

impl RenderConverter {
    pub fn new(device_rate: u32, device_channels: u16, in_channels: u16) -> Self {
        let in_channels = in_channels.clamp(1, 2) as usize;
        Self {
            device_rate,
            device_channels: device_channels.max(1) as usize,
            in_channels,
            resampler: (device_rate != SAMPLE_RATE).then(|| FractionalResampler::new(in_channels)),
            pipeline: vec![0.0; 480 * in_channels],
            resampled: Vec::with_capacity(16_384),
            pending: Vec::with_capacity(16_384),
            pending_pos: 0,
        }
    }

    /// Fill `out` (device format) by calling `pull` for 10 ms 48 kHz blocks as needed.
    pub fn fill(&mut self, out: &mut [f32], pull: &mut dyn FnMut(&mut [f32])) {
        let dev_ch = self.device_channels;
        let mut written = 0;
        while written < out.len() {
            let available = (self.pending.len() - self.pending_pos) / self.in_channels;
            if available == 0 {
                self.refill(pull);
                continue;
            }
            let wanted = (out.len() - written) / dev_ch;
            let frames = available.min(wanted.max(1));
            for f in 0..frames {
                let src =
                    &self.pending[self.pending_pos + f * self.in_channels..][..self.in_channels];
                let dst = &mut out[written + f * dev_ch..][..dev_ch];
                map_channels(src, dst);
            }
            self.pending_pos += frames * self.in_channels;
            written += frames * dev_ch;
            if wanted == 0 {
                break;
            }
        }
    }

    fn refill(&mut self, pull: &mut dyn FnMut(&mut [f32])) {
        pull(&mut self.pipeline);
        self.pending.clear();
        self.pending_pos = 0;
        match self.resampler.as_mut() {
            None => self.pending.extend_from_slice(&self.pipeline),
            Some(r) => {
                self.resampled.clear();
                let ratio = SAMPLE_RATE as f64 / self.device_rate as f64;
                r.process(&self.pipeline, ratio, &mut self.resampled);
                self.pending.extend_from_slice(&self.resampled);
            }
        }
    }
}

fn map_channels(src: &[f32], dst: &mut [f32]) {
    match (src.len(), dst.len()) {
        (a, b) if a == b => dst.copy_from_slice(src),
        (1, _) => dst.fill(src[0]),
        (2, 1) => dst[0] = (src[0] + src[1]) * 0.5,
        (2, _) => {
            dst[0] = src[0];
            dst[1] = src[1];
            dst[2..].fill(0.0);
        }
        _ => dst.fill(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_44k_stereo_to_48k_mono() {
        let mut conv = CaptureConverter::new(44_100, 2, 1);
        let input = vec![0.5f32; 4410 * 2];
        let mut total = 0;
        for _ in 0..10 {
            total += conv.process(&input).len();
        }
        // 1 second at 44.1 kHz → ~48 000 samples at 48 kHz mono.
        assert!((total as i64 - 48_000).abs() < 20, "total {total}");
    }

    #[test]
    fn render_fills_device_buffers_exactly() {
        let mut conv = RenderConverter::new(48_000, 2, 1);
        let mut out = vec![0.0f32; 1024];
        let mut calls = 0;
        conv.fill(&mut out, &mut |buf| {
            calls += 1;
            buf.fill(0.25);
        });
        assert!(out.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        assert_eq!(calls, 2); // 512 frames need two 480-frame pulls
    }

    #[test]
    fn render_resamples_to_44k() {
        let mut conv = RenderConverter::new(44_100, 2, 2);
        let mut pulled = 0usize;
        let mut out = vec![0.0f32; 441 * 2];
        for _ in 0..100 {
            conv.fill(&mut out, &mut |buf| {
                pulled += buf.len() / 2;
                buf.fill(0.1);
            });
        }
        // 1 s of device audio consumes ~48 000 pipeline frames.
        assert!((pulled as i64 - 48_000).abs() <= 960, "pulled {pulled}");
    }
}
