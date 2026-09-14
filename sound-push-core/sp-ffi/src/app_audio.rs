//! Mobile audio backend.
//!
//! - Playback: cpal (AAudio low-latency on Android) by default. With "compatibility output" or
//!   "output audio effects" on, playback moves to the platform path: Kotlin `AudioTrack` pulls
//!   mixed PCM, so the device's own effects and equalizers apply. The platform path is also the
//!   fallback when the fast path cannot open. Running streams switch live when the setting changes.
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
    AudioBackend, AudioError, AudioStream, CaptureCallback, CaptureConverter, CaptureSource,
    DeviceInfo, ErrorCallback, RenderCallback, RenderTarget, StreamInfo,
};
use tracing::warn;

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
        scratch.extend(
            pcm.chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
        );
        for feed in feeds.iter_mut() {
            let converted = feed.converter.process(scratch);
            (feed.callback)(converted);
        }
    }
}

/// Latency reported for platform-path streams (AudioTrack's default buffer is roughly this).
const PLATFORM_LATENCY_MS: u32 = 40;
/// Upper bound for one pull (1 s), so a bad argument cannot allocate without limit.
const MAX_PULL_FRAMES: usize = 48_000;

/// One open render stream. The callback is shared so the stream can move between paths.
struct RenderEntry {
    id: u64,
    target: RenderTarget,
    channels: u16,
    callback: Arc<Mutex<RenderCallback>>,
    on_error: Arc<Mutex<ErrorCallback>>,
    /// `Some` while playing through cpal, `None` while Kotlin pulls it (platform path).
    fast: Option<Box<dyn AudioStream>>,
    info: StreamInfo,
    scratch: Vec<f32>,
}

#[derive(Default)]
struct Renders {
    entries: Vec<RenderEntry>,
    /// The user chose the platform path (compatibility output or output audio effects).
    platform: bool,
    mix: Vec<f32>,
}

type RenderSlot = Arc<Mutex<Renders>>;

fn platform_info(channels: u16) -> StreamInfo {
    StreamInfo {
        device_sample_rate: 48_000,
        device_channels: 2,
        channels,
        latency_ms: PLATFORM_LATENCY_MS,
    }
}

/// Open the low-latency cpal stream for a shared callback. The audio thread never waits for the
/// lock: if a switch holds it, that buffer plays silence.
fn open_fast(
    inner: &CpalBackend,
    target: &RenderTarget,
    channels: u16,
    callback: &Arc<Mutex<RenderCallback>>,
    on_error: &Arc<Mutex<ErrorCallback>>,
) -> Result<Box<dyn AudioStream>, AudioError> {
    let cb = callback.clone();
    let err = on_error.clone();
    inner.open_render(
        target,
        channels,
        Box::new(move |out: &mut [f32]| match cb.try_lock() {
            Ok(mut f) => (f)(out),
            Err(_) => out.fill(0.0),
        }),
        Box::new(move |e| {
            if let Ok(mut f) = err.lock() {
                (f)(e);
            }
        }),
    )
}

pub struct MobileAudioBackend {
    inner: CpalBackend,
    app_audio: Slot,
    mic: Slot,
    renders: RenderSlot,
}

impl MobileAudioBackend {
    pub fn new() -> Self {
        Self {
            inner: CpalBackend::new(),
            app_audio: Slot::default(),
            mic: Slot::default(),
            renders: RenderSlot::default(),
        }
    }

    /// Choose the playback path for new and running streams: `true` hands playback to the
    /// platform (Kotlin AudioTrack), `false` returns to the low-latency path.
    pub fn set_platform_output(&self, enabled: bool) {
        let Ok(mut guard) = self.renders.lock() else {
            return;
        };
        if guard.platform == enabled {
            return;
        }
        guard.platform = enabled;
        for entry in guard.entries.iter_mut() {
            if enabled {
                // Stop the fast stream before Kotlin starts pulling, so audio is never consumed twice.
                drop(entry.fast.take());
                entry.info = platform_info(entry.channels);
            } else if entry.fast.is_none() {
                match open_fast(
                    &self.inner,
                    &entry.target,
                    entry.channels,
                    &entry.callback,
                    &entry.on_error,
                ) {
                    Ok(stream) => {
                        entry.info = stream.info();
                        entry.fast = Some(stream);
                    }
                    Err(e) => {
                        warn!(error = %e, "low-latency output unavailable, staying on platform output")
                    }
                }
            }
        }
    }

    /// True while any stream plays through the platform path (the app should run AudioTrack).
    pub fn platform_output_active(&self) -> bool {
        self.renders
            .lock()
            .map(|g| g.entries.iter().any(|e| e.fast.is_none()))
            .unwrap_or(false)
    }

    /// Mix every platform-path stream into `frames` of 48 kHz interleaved stereo s16le.
    /// Silence when nothing plays there.
    pub fn pull_playback_pcm16(&self, frames: usize) -> Vec<u8> {
        let frames = frames.min(MAX_PULL_FRAMES);
        let mut out = vec![0u8; frames * 4];
        let Ok(mut guard) = self.renders.lock() else {
            return out;
        };
        let Renders { entries, mix, .. } = &mut *guard;
        mix.clear();
        mix.resize(frames * 2, 0.0);
        for entry in entries.iter_mut().filter(|e| e.fast.is_none()) {
            let ch = entry.channels.max(1) as usize;
            entry.scratch.clear();
            entry.scratch.resize(frames * ch, 0.0);
            if let Ok(mut f) = entry.callback.lock() {
                (f)(&mut entry.scratch);
            }
            for (frame, chunk) in entry.scratch.chunks_exact(ch).enumerate() {
                let (l, r) = if ch == 1 {
                    (chunk[0], chunk[0])
                } else {
                    (chunk[0], chunk[1])
                };
                mix[frame * 2] += l;
                mix[frame * 2 + 1] += r;
            }
        }
        for (bytes, sample) in out.chunks_exact_mut(2).zip(mix.iter()) {
            bytes.copy_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        out
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
        self.app_audio
            .lock()
            .map(|g| !g.feeds.is_empty())
            .unwrap_or(false)
    }

    pub fn mic_capture_active(&self) -> bool {
        self.mic
            .lock()
            .map(|g| !g.feeds.is_empty())
            .unwrap_or(false)
    }

    fn open_feed(
        slot: &Slot,
        device_channels: u16,
        channels: u16,
        on_audio: CaptureCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let mut guard = slot
            .lock()
            .map_err(|_| AudioError::Backend("poisoned".into()))?;
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
            CaptureSource::Input(id) if id == APP_AUDIO_DEVICE => {
                Self::open_feed(&self.app_audio, 2, channels, on_audio)
            }
            // Android microphone goes through Kotlin AudioRecord for input presets and effects.
            CaptureSource::DefaultInput | CaptureSource::Input(_)
                if cfg!(target_os = "android") =>
            {
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
        let callback = Arc::new(Mutex::new(on_audio));
        let on_error = Arc::new(Mutex::new(on_error));
        let mut guard = self
            .renders
            .lock()
            .map_err(|_| AudioError::Backend("poisoned".into()))?;
        let fast = if guard.platform {
            None
        } else {
            match open_fast(&self.inner, target, channels, &callback, &on_error) {
                Ok(stream) => Some(stream),
                // Broken low-latency paths exist on some phones: fall back to AudioTrack.
                Err(e) if cfg!(target_os = "android") => {
                    warn!(error = %e, "low-latency output failed, falling back to platform output");
                    None
                }
                Err(e) => return Err(e),
            }
        };
        let id = NEXT_FEED.fetch_add(1, Ordering::Relaxed);
        let info = fast
            .as_ref()
            .map(|s| s.info())
            .unwrap_or_else(|| platform_info(channels));
        guard.entries.push(RenderEntry {
            id,
            target: target.clone(),
            channels,
            callback,
            on_error,
            fast,
            info,
            scratch: Vec::new(),
        });
        Ok(Box::new(RenderHandle {
            renders: self.renders.clone(),
            id,
        }))
    }
}

struct RenderHandle {
    renders: RenderSlot,
    id: u64,
}

impl AudioStream for RenderHandle {
    fn info(&self) -> StreamInfo {
        self.renders
            .lock()
            .ok()
            .and_then(|g| g.entries.iter().find(|e| e.id == self.id).map(|e| e.info))
            .unwrap_or_else(|| platform_info(2))
    }
}

impl Drop for RenderHandle {
    fn drop(&mut self) {
        let removed = self.renders.lock().ok().and_then(|mut g| {
            let pos = g.entries.iter().position(|e| e.id == self.id)?;
            Some(g.entries.remove(pos))
        });
        // Stop the cpal stream (joins its thread) outside the lock.
        drop(removed);
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
            .open_capture(
                &CaptureSource::Input(APP_AUDIO_DEVICE.into()),
                1,
                counting(&received),
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

    #[test]
    fn one_source_feeds_several_routes() {
        let backend = MobileAudioBackend::new();
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let source = CaptureSource::Input(APP_AUDIO_DEVICE.into());
        let first = backend
            .open_capture(&source, 1, counting(&a), Box::new(|_| {}))
            .unwrap();
        let second = backend
            .open_capture(&source, 2, counting(&b), Box::new(|_| {}))
            .unwrap();
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

    fn constant(value: f32) -> RenderCallback {
        Box::new(move |out: &mut [f32]| out.fill(value))
    }

    fn sample(pcm: &[u8], index: usize) -> i16 {
        i16::from_le_bytes([pcm[index * 2], pcm[index * 2 + 1]])
    }

    #[test]
    fn platform_output_mixes_streams_and_releases() {
        let backend = MobileAudioBackend::new();
        backend.set_platform_output(true);
        assert!(!backend.platform_output_active());
        let mono = backend
            .open_render(
                &RenderTarget::DefaultOutput,
                1,
                constant(0.25),
                Box::new(|_| {}),
            )
            .unwrap();
        let stereo = backend
            .open_render(
                &RenderTarget::DefaultOutput,
                2,
                constant(0.25),
                Box::new(|_| {}),
            )
            .unwrap();
        assert!(backend.platform_output_active());
        assert_eq!(mono.info().latency_ms, PLATFORM_LATENCY_MS);

        // Mono is copied to both sides, then the two streams add up: 0.5 on both channels.
        let pcm = backend.pull_playback_pcm16(480);
        assert_eq!(pcm.len(), 480 * 4);
        let expected = (0.5f32 * 32767.0) as i16;
        assert_eq!(sample(&pcm, 0), expected);
        assert_eq!(sample(&pcm, 959), expected);

        drop(mono);
        let pcm = backend.pull_playback_pcm16(10);
        assert_eq!(sample(&pcm, 1), (0.25f32 * 32767.0) as i16);
        drop(stereo);
        assert!(!backend.platform_output_active());
        assert!(backend.pull_playback_pcm16(10).iter().all(|b| *b == 0));
    }

    #[test]
    fn platform_mix_clips_and_bounds_pull_size() {
        let backend = MobileAudioBackend::new();
        backend.set_platform_output(true);
        let _a = backend
            .open_render(
                &RenderTarget::DefaultOutput,
                2,
                constant(0.8),
                Box::new(|_| {}),
            )
            .unwrap();
        let _b = backend
            .open_render(
                &RenderTarget::DefaultOutput,
                2,
                constant(0.8),
                Box::new(|_| {}),
            )
            .unwrap();
        let pcm = backend.pull_playback_pcm16(4);
        assert_eq!(sample(&pcm, 0), i16::MAX);
        assert_eq!(
            backend.pull_playback_pcm16(usize::MAX).len(),
            MAX_PULL_FRAMES * 4
        );
    }
}
