//! Null backend for tests and headless environments.
//!
//! Capture produces a generated signal; render discards (or records) audio.
//! Streams tick on a normal thread at real time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{
    AudioBackend, AudioError, AudioStream, CaptureCallback, CaptureSource, DeviceInfo, DeviceKind, ErrorCallback,
    RenderCallback, RenderTarget, StreamInfo,
};

#[derive(Default)]
pub struct NullBackend {
    /// When set, rendered audio is appended here (tests).
    pub recorded: Option<Arc<Mutex<Vec<f32>>>>,
    /// Frequency of the generated capture sine (0 = silence).
    pub capture_frequency: f32,
}

struct NullStream {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    info: StreamInfo,
}

impl AudioStream for NullStream {
    fn info(&self) -> StreamInfo {
        self.info
    }
}

impl Drop for NullStream {
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

impl AudioBackend for NullBackend {
    fn name(&self) -> &'static str {
        "null"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(vec![
            DeviceInfo {
                id: "null-in".into(),
                name: "Null input".into(),
                kind: DeviceKind::Input,
                is_default: true,
                channels: 2,
                sample_rate: 48_000,
            },
            DeviceInfo {
                id: "null-out".into(),
                name: "Null output".into(),
                kind: DeviceKind::Output,
                is_default: true,
                channels: 2,
                sample_rate: 48_000,
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
        let freq = self.capture_frequency;
        let mut buf = vec![0.0f32; 480 * ch];
        let (running, thread) = spawn_ticker(move |n| {
            for (i, frame) in buf.chunks_exact_mut(ch).enumerate() {
                let t = (n * 480 + i as u64) as f32 / 48_000.0;
                let v = if freq > 0.0 { (2.0 * std::f32::consts::PI * freq * t).sin() * 0.3 } else { 0.0 };
                frame.fill(v);
            }
            on_audio(&buf);
        });
        Ok(Box::new(NullStream {
            running,
            thread: Some(thread),
            info: StreamInfo {
                device_sample_rate: 48_000,
                device_channels: channels,
                channels,
                latency_ms: 10,
            },
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
        let mut buf = vec![0.0f32; 480 * ch];
        let (running, thread) = spawn_ticker(move |_| {
            on_audio(&mut buf);
            if let Some(rec) = &recorded {
                if let Ok(mut r) = rec.lock() {
                    r.extend_from_slice(&buf);
                }
            }
        });
        Ok(Box::new(NullStream {
            running,
            thread: Some(thread),
            info: StreamInfo {
                device_sample_rate: 48_000,
                device_channels: channels,
                channels,
                latency_ms: 10,
            },
        }))
    }
}
