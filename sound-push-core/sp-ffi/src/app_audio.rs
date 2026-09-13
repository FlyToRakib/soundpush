//! Mobile audio backend.
//!
//! - Playback: cpal (AAudio on Android).
//! - Microphone (Android): Kotlin `AudioRecord` with the user's input preset and
//!   platform effects (echo cancellation, noise suppression, AGC), pushed in as PCM.
//! - App audio (Android 10+): Kotlin playback-capture `AudioRecord`, pushed in as PCM.
//!
//! Kotlin learns when to run each recorder from `mic_capture_active` /
//! `app_audio_active` (and the engine's keep-alive callback).

use std::sync::{Arc, Mutex};

use sp_audio_io::cpal_backend::CpalBackend;
use sp_audio_io::{
    AudioBackend, AudioError, AudioStream, CaptureCallback, CaptureConverter, CaptureSource, DeviceInfo,
    ErrorCallback, RenderCallback, RenderTarget, StreamInfo,
};

/// Capture device id used for Android playback capture.
pub const APP_AUDIO_DEVICE: &str = "android-app-audio";

struct Feed {
    callback: CaptureCallback,
    converter: CaptureConverter,
    scratch: Vec<f32>,
}

type Slot = Arc<Mutex<Option<Feed>>>;

fn push_pcm16(slot: &Slot, pcm: &[u8]) {
    if let Ok(mut guard) = slot.lock() {
        if let Some(feed) = guard.as_mut() {
            feed.scratch.clear();
            feed.scratch
                .extend(pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0));
            let Feed {
                callback,
                converter,
                scratch,
            } = feed;
            let converted = converter.process(scratch);
            callback(converted);
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
            app_audio: Arc::new(Mutex::new(None)),
            mic: Arc::new(Mutex::new(None)),
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
        self.app_audio.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    pub fn mic_capture_active(&self) -> bool {
        self.mic.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    fn open_feed(slot: &Slot, device_channels: u16, channels: u16, on_audio: CaptureCallback) -> Result<Box<dyn AudioStream>, AudioError> {
        let mut guard = slot.lock().map_err(|_| AudioError::Backend("poisoned".into()))?;
        if guard.is_some() {
            return Err(AudioError::DeviceBusy);
        }
        *guard = Some(Feed {
            callback: on_audio,
            converter: CaptureConverter::new(48_000, device_channels, channels),
            scratch: Vec::with_capacity(8192),
        });
        Ok(Box::new(FeedStream {
            slot: slot.clone(),
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
            *guard = None;
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn app_audio_feed_converts_and_releases() {
        let backend = MobileAudioBackend::new();
        let received = Arc::new(AtomicUsize::new(0));
        let r = received.clone();
        let stream = backend
            .open_capture(
                &CaptureSource::Input(APP_AUDIO_DEVICE.into()),
                1,
                Box::new(move |s: &[f32]| {
                    r.fetch_add(s.len(), Ordering::Relaxed);
                }),
                Box::new(|_| {}),
            )
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
}
