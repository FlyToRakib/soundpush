//! Mobile audio backend.
//!
//! - Playback: cpal (AAudio on Android).
//! - Microphone (Android): Kotlin `AudioRecord` with the user's input preset and
//!   platform effects (echo cancellation, noise suppression, AGC), pushed in as PCM.
//! - App audio (Android 10+): Kotlin playback-capture `AudioRecord`, pushed in as PCM.
//!
//! One recorder can feed several routes at once (e.g. the microphone sent to two
//! computers), so each source fans its PCM out to every open stream.
//!
//! Kotlin learns when to run each recorder from `mic_capture_active` /
//! `app_audio_active` (and the engine's keep-alive callback).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sp_audio_io::cpal_backend::CpalBackend;
use sp_audio_io::{
    AudioBackend, AudioError, AudioStream, CaptureCallback, CaptureConverter, CaptureSource, DeviceInfo,
    ErrorCallback, RenderCallback, RenderTarget, StreamInfo,
};

/// Capture device id used for Android playback capture.
pub const APP_AUDIO_DEVICE: &str = "android-app-audio";

struct Feed {
    id: u64,
    callback: CaptureCallback,
    converter: CaptureConverter,
}

#[derive(Default)]
struct Source {
    feeds: Vec<Feed>,
    scratch: Vec<f32>,
}

type Slot = Arc<Mutex<Source>>;

static NEXT_FEED: AtomicU64 = AtomicU64::new(1);

fn push_pcm16(slot: &Slot, pcm: &[u8]) {
    if let Ok(mut guard) = slot.lock() {
        let Source { feeds, scratch } = &mut *guard;
        if feeds.is_empty() {
            return;
        }
        scratch.clear();
        scratch.extend(pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0));
        for feed in feeds.iter_mut() {
            let converted = feed.converter.process(scratch);
            (feed.callback)(converted);
        }
    }
}

pub struct MobileAudioBackend {
    inner: CpalBackend,
    app_audio: Slot,
    mic: Slot,
}

impl MobileAudioBackend {
    pub fn new() -> Self {
        Self {
            inner: CpalBackend::new(),
            app_audio: Slot::default(),
            mic: Slot::default(),
        }
    }

    /// 48 kHz interleaved stereo s16le from playback capture.
    pub fn push_app_audio_pcm16(&self, pcm: &[u8]) {
        push_pcm16(&self.app_audio, pcm);
    }

    /// 48 kHz mono s16le from the microphone recorder.
    pub fn push_mic_pcm16(&self, pcm: &[u8]) {
        push_pcm16(&self.mic, pcm);
    }

    pub fn app_audio_active(&self) -> bool {
        self.app_audio.lock().map(|g| !g.feeds.is_empty()).unwrap_or(false)
    }

    pub fn mic_capture_active(&self) -> bool {
        self.mic.lock().map(|g| !g.feeds.is_empty()).unwrap_or(false)
    }

    fn open_feed(slot: &Slot, device_channels: u16, channels: u16, on_audio: CaptureCallback) -> Result<Box<dyn AudioStream>, AudioError> {
        let mut guard = slot.lock().map_err(|_| AudioError::Backend("poisoned".into()))?;
        let id = NEXT_FEED.fetch_add(1, Ordering::Relaxed);
        guard.feeds.push(Feed {
            id,
            callback: on_audio,
            converter: CaptureConverter::new(48_000, device_channels, channels),
        });
        Ok(Box::new(FeedStream {
            slot: slot.clone(),
            id,
            device_channels,
            channels,
        }))
    }
}

impl Default for MobileAudioBackend {
    fn default() -> Self {
        Self::new()
    }
}

struct FeedStream {
    slot: Slot,
    id: u64,
    device_channels: u16,
    channels: u16,
}

impl AudioStream for FeedStream {
    fn info(&self) -> StreamInfo {
        StreamInfo {
            device_sample_rate: 48_000,
            device_channels: self.device_channels,
            channels: self.channels,
            latency_ms: 20,
        }
    }
}

impl Drop for FeedStream {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.slot.lock() {
            guard.feeds.retain(|f| f.id != self.id);
        }
    }
}

impl AudioBackend for MobileAudioBackend {
    fn name(&self) -> &'static str {
        "mobile"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        self.inner.list_devices()
    }

    fn supports_loopback(&self) -> bool {
        false
    }

    fn open_capture(
        &self,
        source: &CaptureSource,
        channels: u16,
        on_audio: CaptureCallback,
        on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        match source {
            CaptureSource::Input(id) if id == APP_AUDIO_DEVICE => Self::open_feed(&self.app_audio, 2, channels, on_audio),
            // Android microphone goes through Kotlin AudioRecord for input presets and effects.
            CaptureSource::DefaultInput | CaptureSource::Input(_) if cfg!(target_os = "android") => {
                Self::open_feed(&self.mic, 1, channels, on_audio)
            }
            other => self.inner.open_capture(other, channels, on_audio, on_error),
        }
    }

    fn open_render(
        &self,
        target: &RenderTarget,
        channels: u16,
        on_audio: RenderCallback,
        on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        self.inner.open_render(target, channels, on_audio, on_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn counting(counter: &Arc<AtomicUsize>) -> CaptureCallback {
        let c = counter.clone();
        Box::new(move |s: &[f32]| {
            c.fetch_add(s.len(), Ordering::Relaxed);
        })
    }

    #[test]
    fn app_audio_feed_converts_and_releases() {
        let backend = MobileAudioBackend::new();
        let received = Arc::new(AtomicUsize::new(0));
        let stream = backend
            .open_capture(&CaptureSource::Input(APP_AUDIO_DEVICE.into()), 1, counting(&received), Box::new(|_| {}))
            .unwrap();
        assert!(backend.app_audio_active());
        // 10 ms stereo s16le = 1920 bytes → 480 mono samples.
        backend.push_app_audio_pcm16(&vec![0u8; 1920]);
        assert_eq!(received.load(Ordering::Relaxed), 480);
        drop(stream);
        assert!(!backend.app_audio_active());
        backend.push_app_audio_pcm16(&vec![0u8; 1920]);
        assert_eq!(received.load(Ordering::Relaxed), 480);
    }

    #[test]
    fn one_source_feeds_several_routes() {
        let backend = MobileAudioBackend::new();
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let source = CaptureSource::Input(APP_AUDIO_DEVICE.into());
        let first = backend.open_capture(&source, 1, counting(&a), Box::new(|_| {})).unwrap();
        let second = backend.open_capture(&source, 2, counting(&b), Box::new(|_| {})).unwrap();
        backend.push_app_audio_pcm16(&vec![0u8; 1920]);
        assert_eq!(a.load(Ordering::Relaxed), 480);
        assert_eq!(b.load(Ordering::Relaxed), 960);

        // Stopping one route leaves the other running.
        drop(first);
        assert!(backend.app_audio_active());
        backend.push_app_audio_pcm16(&vec![0u8; 1920]);
        assert_eq!(a.load(Ordering::Relaxed), 480);
        assert_eq!(b.load(Ordering::Relaxed), 1920);
        drop(second);
        assert!(!backend.app_audio_active());
    }
}
