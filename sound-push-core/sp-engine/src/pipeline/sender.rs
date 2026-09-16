//! Sender pipeline: capture → DSP → encode → media datagrams, fanned out to every subscriber.
//!
//! One [`Sender`] is an *encoder group* (plan §15.2): a single capture and encoder for a
//! (source, profile) pair. Each route that can use it subscribes with its own route id, sink
//! and per-route controls, so N receivers of the same stream cost one encode.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use sp_audio_io::{AudioBackend, AudioStream, CaptureSource, ErrorCallback};
use sp_media::codec::{Encoder, OpusApplication, encoder_for};
use sp_media::drift::{DriftController, FractionalResampler};
use sp_media::dsp::{
    DuckGate, Gain, HighPass, LevelMeter, NoiseSuppressor, SoftLimiter, db_to_gain, is_silent,
};
use sp_media::samples_per_frame;
use sp_protocol::control::StreamProfile;
use sp_protocol::{Codec, MediaFlags, MediaHeader, MediaPacket};
use tracing::{debug, warn};

use super::controls::{EchoReference, SenderControls};
use crate::EngineError;

/// Silence (every sample below −60 dBFS) lasting this long starts DTX. The hangover keeps word
/// endings and short pauses as real audio.
const DTX_HANGOVER_MS: u64 = 200;
/// DTX packets go out for this many frames when silence starts and then once per keep-alive
/// interval, so a receiver that lost the first ones still learns the sender is silent.
const DTX_SIGNAL_FRAMES: u64 = 2;
const DTX_KEEPALIVE_MS: u64 = 400;

/// A datagram could not be handed to the transport (closed, congested or malformed).
/// Media is best effort: callers count these and move on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("datagram not sent")]
pub struct SendFailed;

/// Something that can carry media datagrams (a connection, or a test sink).
pub trait DatagramSink: Send + Sync + 'static {
    fn send(&self, datagram: Bytes) -> Result<(), SendFailed>;
}

impl DatagramSink for sp_transport::SecureConnection {
    fn send(&self, datagram: Bytes) -> Result<(), SendFailed> {
        self.send_datagram(datagram).map_err(|_| SendFailed)
    }
}

pub struct SenderConfig {
    pub profile: StreamProfile,
    pub application: OpusApplication,
    pub source: CaptureSource,
    /// What this device plays on its own speakers, for the echo duck (microphone groups).
    pub echo: Option<Arc<EchoReference>>,
    /// Mixed source (plan §5.1): a second capture summed into the first, each part with its own
    /// gain. `None` for every single-source stream, which then takes the path it always did.
    pub mix: Option<CaptureSource>,
}

/// One route receiving a group's packets.
pub struct Subscriber {
    pub route: u8,
    pub sink: Arc<dyn DatagramSink>,
    /// The receiver understands DTX packets (`FEATURE_DTX`); others always get encoded audio.
    pub dtx: bool,
    /// Per-route controls and statistics: `muted`, `bitrate`, `expected_loss_pct`,
    /// `redundancy`, `frames_sent`, `bytes_sent`, `send_errors`; `level_db` mirrors the group.
    pub controls: Arc<SenderControls>,
}

struct Entry {
    id: u64,
    subscriber: Subscriber,
}

#[derive(Default)]
struct Fanout {
    entries: Mutex<Vec<Entry>>,
    /// Bumped on every change so the encode thread re-reads the list only when needed.
    generation: AtomicU64,
    next_id: AtomicU64,
}

impl Fanout {
    fn remove(&self, id: u64) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|e| e.id != id);
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

/// Keeps a route subscribed; dropping it unsubscribes.
pub struct Subscription {
    fanout: Arc<Fanout>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.fanout.remove(self.id);
    }
}

pub struct Sender {
    _capture: Box<dyn AudioStream>,
    /// Second capture of a mixed source (plan §5.1); `None` for every other stream.
    _mix_capture: Option<Box<dyn AudioStream>>,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    fanout: Arc<Fanout>,
    /// Group-wide controls: `gain_db`, `noise_suppression`, `muted` (microphone mute),
    /// `level_db`, `clipping`, `capture_overruns`, and for a mixed source the two part gains.
    pub controls: Arc<SenderControls>,
}

impl Sender {
    /// Open the capture and start encoding. Blocks while the audio device opens, so the engine
    /// calls it off its actor task.
    pub fn start(
        backend: &dyn AudioBackend,
        config: SenderConfig,
        controls: Arc<SenderControls>,
        on_error: ErrorCallback,
    ) -> Result<Self, EngineError> {
        let channels = config.profile.channels.clamp(1, 2) as usize;
        let frame = samples_per_frame(config.profile.frame_us);
        let mut encoder = encoder_for(&config.profile, config.application)
            .map_err(|e| EngineError::Internal(e.to_string()))?;

        // One second of capture headroom.
        let (producer, mut consumer) = rtrb::RingBuffer::<f32>::new(48_000 * channels);
        let capture = backend.open_capture(
            &config.source,
            channels as u16,
            fill_ring(producer, controls.clone()),
            on_error,
        )?;

        // The mixed source's second capture runs on its own clock, so it gets its own ring and
        // the encode thread takes what is there when it needs a frame. Its failures are reported
        // like the first capture's: the group closes and the routes stop or reopen.
        let (mix_capture, mix_consumer) = match &config.mix {
            Some(source) => {
                let (producer, consumer) = rtrb::RingBuffer::<f32>::new(48_000 * channels);
                let stream = backend.open_capture(
                    source,
                    channels as u16,
                    fill_ring(producer, controls.clone()),
                    Box::new(|_| {}),
                )?;
                (Some(stream), Some(consumer))
            }
            None => (None, None),
        };

        let running = Arc::new(AtomicBool::new(true));
        let fanout = Arc::new(Fanout::default());
        let thread = {
            let running = running.clone();
            let controls = controls.clone();
            let fanout = fanout.clone();
            let codec = encoder.codec();
            let echo = config.echo.clone();
            let mut mix_consumer = mix_consumer;
            std::thread::Builder::new()
                .name("sp-encode".into())
                .spawn(move || {
                    encode_loop(
                        &running,
                        &controls,
                        &fanout,
                        &mut consumer,
                        mix_consumer.as_mut(),
                        encoder.as_mut(),
                        codec,
                        channels,
                        frame,
                        echo.as_deref(),
                    )
                })
                .map_err(|e| EngineError::Internal(e.to_string()))?
        };

        Ok(Self {
            _capture: capture,
            _mix_capture: mix_capture,
            running,
            thread: Some(thread),
            fanout,
            controls,
        })
    }

    /// Add a route to this group.
    pub fn subscribe(&self, subscriber: Subscriber) -> Subscription {
        let id = self.fanout.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut entries) = self.fanout.entries.lock() {
            entries.push(Entry { id, subscriber });
        }
        self.fanout.generation.fetch_add(1, Ordering::Relaxed);
        Subscription {
            fanout: self.fanout.clone(),
            id,
        }
    }

    pub fn subscribers(&self) -> usize {
        self.fanout.entries.lock().map(|e| e.len()).unwrap_or(0)
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Reads a mixed source's second capture a frame at a time.
///
/// The two captures run on their own clocks, and their callbacks reach the encode thread with
/// ordinary scheduling jitter: at the moment the first capture's frame is ready, the second is
/// often a few samples short. Taking whatever happens to be there and padding the rest with
/// zeros turns that jitter into a gap in the voice every few frames. Instead the reader keeps
/// [`MIX_PREFILL_FRAMES`] in hand before it starts — about 20 ms, on the microphone half only —
/// and only ever takes whole frames. If the capture does run dry (it stalled, or its clock is
/// genuinely stalled) it plays one silent frame and fills the cushion again, rather than clicking
/// on every late callback.
///
/// Two audio devices never run at exactly the same rate — a USB microphone against the sound
/// card's loopback differs by tens to hundreds of ppm — so a cushion on its own would still drain
/// or overflow every few minutes, with a dropout or a skip each time. The reader therefore also
/// resamples the microphone by a hair, steered by the cushion's fill ([`DriftController`], the
/// same compensation the receiver uses), and the fill stays where it is. Backlog beyond
/// [`MIX_MAX_BACKLOG_FRAMES`] is still dropped as a last resort.
struct MixReader {
    primed: bool,
    channels: usize,
    drift: DriftController,
    resampler: FractionalResampler,
    /// Microphone samples read from the ring, waiting to be resampled.
    scratch: Vec<f32>,
    /// Resampled microphone audio not yet mixed.
    pending: Vec<f32>,
}

impl MixReader {
    fn new(channels: usize) -> Self {
        let channels = channels.max(1);
        Self {
            primed: false,
            channels,
            drift: DriftController::new(),
            resampler: FractionalResampler::new(channels),
            scratch: Vec::new(),
            pending: Vec::new(),
        }
    }

    fn take(&mut self, input: &mut rtrb::Consumer<f32>, out: &mut [f32]) {
        let frame = out.len();
        let keep = frame * MIX_MAX_BACKLOG_FRAMES;
        if input.slots() > keep {
            let stale = input.slots() - keep;
            if let Ok(chunk) = input.read_chunk(stale) {
                chunk.commit_all();
            }
        }
        let available = input.slots();
        if !self.primed && available < frame * MIX_PREFILL_FRAMES {
            out.fill(0.0);
            return;
        }
        // Whole interleaved frames only, so channels never swap.
        let ch = self.channels;
        let whole = |n: usize| n - n % ch;
        // Measured before this frame is taken, so the target includes it: what the controller
        // holds on to is the full cushion left once the frame is gone.
        let ratio = self.drift.update(
            (available + self.pending.len()) as u64,
            (frame * (MIX_PREFILL_FRAMES + 1)) as u64,
        );
        // What resampling at `ratio` needs to produce the rest of this frame, plus the
        // interpolation history the first time round. Any sample it comes up short is topped up
        // below, so this asks for no spare: every sample held back here is cushion lost.
        let wanted = frame.saturating_sub(self.pending.len());
        let mut need = whole((wanted as f64 * ratio).round() as usize);
        if !self.primed {
            need += 3 * self.channels;
        }
        if available < need {
            self.underrun(out);
            return;
        }
        self.read(input, need);
        // Hermite interpolation keeps a sample or two back; top up one sample frame at a time
        // until the frame is whole, as long as the ring has them.
        while self.pending.len() < frame {
            if input.slots() < self.channels {
                self.underrun(out);
                return;
            }
            self.read(input, self.channels);
        }
        self.primed = true;
        out.copy_from_slice(&self.pending[..frame]);
        self.pending.drain(..frame);
    }

    fn read(&mut self, input: &mut rtrb::Consumer<f32>, n: usize) {
        self.scratch.clear();
        if let Ok(chunk) = input.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            self.scratch.extend_from_slice(a);
            self.scratch.extend_from_slice(b);
            chunk.commit_all();
        }
        self.resampler
            .process(&self.scratch, self.drift.ratio(), &mut self.pending);
    }

    /// The microphone ran dry: one silent frame, and start over from a fresh cushion.
    fn underrun(&mut self, out: &mut [f32]) {
        self.primed = false;
        self.pending.clear();
        self.resampler.reset();
        self.drift.reset();
        out.fill(0.0);
    }
}

/// A capture callback that writes into `producer`, counting what a full ring had to drop.
fn fill_ring(
    mut producer: rtrb::Producer<f32>,
    controls: Arc<SenderControls>,
) -> sp_audio_io::CaptureCallback {
    Box::new(move |samples: &[f32]| {
        let n = samples.len().min(producer.slots());
        if n < samples.len() {
            controls.capture_overruns.fetch_add(1, Ordering::Relaxed);
        }
        if let Ok(chunk) = producer.write_chunk_uninit(n) {
            // Safe, allocation-free copy that commits exactly what it wrote.
            chunk.fill_from_iter(samples.iter().copied());
        }
    })
}

/// The encode thread's copy of a subscriber.
struct Target {
    id: u64,
    route: u8,
    sink: Arc<dyn DatagramSink>,
    dtx: bool,
    controls: Arc<SenderControls>,
    /// Next packet starts (or restarts) the stream for this receiver.
    restart: bool,
    first: bool,
}

fn refresh_targets(fanout: &Fanout, targets: &mut Vec<Target>) {
    let Ok(entries) = fanout.entries.lock() else {
        return;
    };
    let mut next = Vec::with_capacity(entries.len());
    for e in entries.iter() {
        let (restart, first) = targets
            .iter()
            .find(|t| t.id == e.id)
            .map_or((false, true), |t| (t.restart, t.first));
        next.push(Target {
            id: e.id,
            route: e.subscriber.route,
            sink: e.subscriber.sink.clone(),
            dtx: e.subscriber.dtx,
            controls: e.subscriber.controls.clone(),
            restart,
            first,
        });
    }
    *targets = next;
}

/// A mixed source's second capture may not be exactly in step with the first. More than this
/// much waiting audio is dropped, so the microphone part cannot drift behind the system audio.
/// Drift compensation keeps the fill near the cushion, so this is only reached when a capture
/// delivers a burst after stalling; it leaves room for the cushion plus a late callback's catch-up.
const MIX_MAX_BACKLOG_FRAMES: usize = 6;
/// Frames of the second capture held back before the mix takes from it, and again after it has
/// run dry: enough to ride out the ordinary scheduling jitter between two capture clocks.
const MIX_PREFILL_FRAMES: usize = 2;

#[allow(clippy::too_many_arguments)]
fn encode_loop(
    running: &AtomicBool,
    controls: &SenderControls,
    fanout: &Fanout,
    input: &mut rtrb::Consumer<f32>,
    mut mix_input: Option<&mut rtrb::Consumer<f32>>,
    encoder: &mut dyn Encoder,
    codec: Codec,
    channels: usize,
    frame: usize,
    echo: Option<&EchoReference>,
) {
    let mut mix_buf = vec![0.0f32; frame * channels];
    let mut mix_reader = MixReader::new(channels);
    let mut mix_gain = Gain::new(db_to_gain(controls.mix_gain_db.get()));
    let mut system_gain = Gain::new(db_to_gain(controls.system_gain_db.get()));
    let mut buf = vec![0.0f32; frame * channels];
    let mut packet = Vec::with_capacity(1500);
    let mut previous: Vec<u8> = Vec::with_capacity(1500);
    let mut payload: Vec<u8> = Vec::with_capacity(1500);
    let mut gain = Gain::new(db_to_gain(controls.gain_db.get()));
    let limiter = SoftLimiter::default();
    let mut meter = LevelMeter::default();
    let mut denoiser: Option<NoiseSuppressor> = None;
    let mut high_pass: Option<HighPass> = None;
    let mut duck = DuckGate::new();
    let mut applied_bitrate = 0u32;
    let mut applied_loss = u32::MAX;
    let mut timestamp: u64 = 0;
    let mut seq: u32 = 0;
    let mut targets: Vec<Target> = Vec::new();
    let mut generation = u64::MAX;
    let dtx_hangover = DTX_HANGOVER_MS * 48 / frame.max(1) as u64;
    let dtx_keepalive = (DTX_KEEPALIVE_MS * 48 / frame.max(1) as u64).max(1);
    let mut silent_frames: u64 = 0;

    while running.load(Ordering::Relaxed) {
        if input.slots() < buf.len() {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        let Ok(chunk) = input.read_chunk(buf.len()) else {
            continue;
        };
        let (a, b) = chunk.as_slices();
        buf[..a.len()].copy_from_slice(a);
        buf[a.len()..].copy_from_slice(b);
        chunk.commit_all();

        let current = fanout.generation.load(Ordering::Relaxed);
        if current != generation {
            refresh_targets(fanout, &mut targets);
            generation = current;
        }

        // Live control changes. A shared encoder follows its most constrained receiver.
        if let Some(bitrate) = targets
            .iter()
            .map(|t| t.controls.bitrate.load(Ordering::Relaxed))
            .min()
        {
            if bitrate != applied_bitrate {
                encoder.set_bitrate(bitrate);
                applied_bitrate = bitrate;
            }
        }
        let loss = targets
            .iter()
            .map(|t| t.controls.expected_loss_pct.load(Ordering::Relaxed))
            .max()
            .unwrap_or(0);
        if loss != applied_loss {
            encoder.set_expected_loss(loss.min(100) as u8);
            applied_loss = loss;
        }
        gain.set(db_to_gain(controls.gain_db.get()));

        // A mixed source (plan §5.1) takes one frame of microphone alongside the system audio.
        let mixing = if let Some(mic) = mix_input.as_mut() {
            mix_reader.take(mic, &mut mix_buf);
            true
        } else {
            false
        };

        // DSP (plan §15.7): high-pass → noise suppression → gain → limiter → meter. In a mixed
        // stream that chain belongs to the microphone part; the system audio arrives ready.
        {
            let dsp = if mixing { &mut mix_buf } else { &mut buf };
            if controls.high_pass.load(Ordering::Relaxed) {
                high_pass
                    .get_or_insert_with(|| HighPass::new(80.0, channels))
                    .process(dsp);
            } else {
                high_pass = None;
            }
            if controls.noise_suppression.load(Ordering::Relaxed)
                && channels == 1
                && frame % 480 == 0
            {
                denoiser
                    .get_or_insert_with(NoiseSuppressor::new)
                    .process(dsp);
            } else {
                denoiser = None;
            }
            gain.process(dsp);
            // Half-duplex echo control: while this device plays the other side on its speakers,
            // the microphone is lowered so the other side does not hear itself back (plan §15.7).
            // In a mixed stream only the microphone part is ducked; the system audio is not echo.
            if let Some(echo) = echo
                .as_ref()
                .filter(|_| controls.echo_ducking.load(Ordering::Relaxed))
            {
                duck.process(dsp, echo.level_db());
            }
        }
        if mixing {
            // Each part keeps its own gain, so the balance between them is the user's (plan §5.1).
            // The microphone mute silences only its half; the system audio keeps playing.
            if controls.mix_muted.load(Ordering::Relaxed) {
                mix_buf.fill(0.0);
            } else {
                mix_gain.set(db_to_gain(controls.mix_gain_db.get()));
                mix_gain.process(&mut mix_buf);
            }
            system_gain.set(db_to_gain(controls.system_gain_db.get()));
            system_gain.process(&mut buf);
            for (out, mic) in buf.iter_mut().zip(mix_buf.iter()) {
                *out += *mic;
            }
        }
        // The limiter catches a sum that went over full scale.
        let clipped = limiter.process(&mut buf);
        meter.process(&buf);
        let level = meter.peak_db();
        controls.level_db.set(level);
        controls.clipping.store(clipped, Ordering::Relaxed);
        if controls.muted.load(Ordering::Relaxed) {
            buf.fill(0.0);
        }

        let this_ts = timestamp;
        let this_seq = seq;
        timestamp += frame as u64;
        seq = seq.wrapping_add(1);
        // Silence detection / DTX (plan §15.2), Opus only: lossless PCM sends every frame.
        // After the hangover, receivers that understand DTX get header-only DTX packets instead
        // of encoded silence.
        silent_frames = if is_silent(&buf) {
            silent_frames.saturating_add(1)
        } else {
            0
        };
        let dtx = codec == Codec::Opus && silent_frames > dtx_hangover;
        let dtx_signal = dtx && {
            let n = silent_frames - dtx_hangover - 1;
            n < DTX_SIGNAL_FRAMES || n % dtx_keepalive == 0
        };
        if targets.is_empty() {
            previous.clear();
            continue;
        }

        let needs_audio = !dtx || targets.iter().any(|t| !t.dtx);
        if needs_audio && let Err(e) = encoder.encode(&buf, &mut packet) {
            warn!(error = %e, "encode failed");
            continue;
        }

        // Wire bytes are built once per variant (with and without redundancy) and copied per
        // receiver with its route id and flags patched in.
        let mut plain: Option<Bytes> = None;
        let mut redundant: Option<Bytes> = None;
        let mut silence: Option<Bytes> = None;
        let can_add_redundancy = codec == Codec::Opus
            && needs_audio
            && !previous.is_empty()
            && 2 + packet.len() + previous.len() <= sp_protocol::media::MAX_MEDIA_PAYLOAD;
        for t in &mut targets {
            t.controls.level_db.set(level);
            if t.controls.muted.load(Ordering::Relaxed) {
                // Nothing is sent to a muted receiver; it resynchronizes when unmuted.
                t.restart = true;
                continue;
            }
            let silent_target = dtx && t.dtx;
            if silent_target && !dtx_signal {
                continue;
            }
            let with_redundancy = !silent_target
                && can_add_redundancy
                && t.controls.redundancy.load(Ordering::Relaxed);
            let slot = if silent_target {
                &mut silence
            } else if with_redundancy {
                &mut redundant
            } else {
                &mut plain
            };
            if slot.is_none() {
                payload.clear();
                let mut flags = MediaFlags::empty();
                if silent_target {
                    // DTX: the header alone says "silent from this timestamp"; no payload.
                    flags = flags.with(MediaFlags::DTX);
                } else if with_redundancy {
                    // Redundancy (Opus only): [u16 primary length][primary][previous frame].
                    flags = flags.with(MediaFlags::REDUNDANT);
                    payload.extend_from_slice(&(packet.len() as u16).to_be_bytes());
                    payload.extend_from_slice(&packet);
                    payload.extend_from_slice(&previous);
                } else {
                    payload.extend_from_slice(&packet);
                }
                let media = MediaPacket {
                    header: MediaHeader {
                        route: 0,
                        codec,
                        flags,
                        sample_timestamp: this_ts,
                        seq: this_seq,
                    },
                    payload: Bytes::copy_from_slice(&payload),
                };
                match media.encode() {
                    Ok(wire) => *slot = Some(wire),
                    Err(e) => {
                        debug!(error = %e, "packet too large");
                        continue;
                    }
                }
            }
            let Some(wire) = slot.as_ref() else { continue };
            let mut out = BytesMut::from(&wire[..]);
            out[1] = t.route;
            if t.first {
                out[3] |= MediaFlags::MARKER;
            }
            if t.restart {
                out[3] |= MediaFlags::DISCONTINUITY;
            }
            let len = out.len() as u64;
            if t.sink.send(out.freeze()).is_ok() {
                t.first = false;
                t.restart = false;
                t.controls.frames_sent.fetch_add(1, Ordering::Relaxed);
                t.controls.bytes_sent.fetch_add(len, Ordering::Relaxed);
            } else {
                t.controls.send_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        previous.clear();
        if needs_audio {
            previous.extend_from_slice(&packet);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use sp_audio_io::null::NullBackend;
    use sp_media::profile::{LatencyProfile, Quality, build_profile};

    use super::*;

    #[derive(Default)]
    struct Collect(Mutex<Vec<Bytes>>);

    impl DatagramSink for Collect {
        fn send(&self, datagram: Bytes) -> Result<(), SendFailed> {
            if let Ok(mut v) = self.0.lock() {
                v.push(datagram);
            }
            Ok(())
        }
    }

    /// Counts opened capture streams.
    struct Counting(NullBackend, AtomicUsize);

    impl AudioBackend for Counting {
        fn name(&self) -> &'static str {
            "counting"
        }
        fn list_devices(&self) -> Result<Vec<sp_audio_io::DeviceInfo>, sp_audio_io::AudioError> {
            self.0.list_devices()
        }
        fn supports_loopback(&self) -> bool {
            true
        }
        fn open_capture(
            &self,
            source: &CaptureSource,
            channels: u16,
            on_audio: sp_audio_io::CaptureCallback,
            on_error: ErrorCallback,
        ) -> Result<Box<dyn AudioStream>, sp_audio_io::AudioError> {
            self.1.fetch_add(1, Ordering::Relaxed);
            self.0.open_capture(source, channels, on_audio, on_error)
        }
        fn open_render(
            &self,
            target: &sp_audio_io::RenderTarget,
            channels: u16,
            on_audio: sp_audio_io::RenderCallback,
            on_error: ErrorCallback,
        ) -> Result<Box<dyn AudioStream>, sp_audio_io::AudioError> {
            self.0.open_render(target, channels, on_audio, on_error)
        }
    }

    /// Two captures at different, constant levels, so a mixed stream can be checked sample by
    /// sample: system audio at 0.4, the microphone at 0.2.
    struct TwoSources;

    struct Ticker {
        running: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl AudioStream for Ticker {
        fn info(&self) -> sp_audio_io::StreamInfo {
            sp_audio_io::StreamInfo {
                device_sample_rate: 48_000,
                device_channels: 2,
                channels: 2,
                latency_ms: 10,
            }
        }
    }

    impl Drop for Ticker {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl AudioBackend for TwoSources {
        fn name(&self) -> &'static str {
            "two-sources"
        }
        fn list_devices(&self) -> Result<Vec<sp_audio_io::DeviceInfo>, sp_audio_io::AudioError> {
            Ok(Vec::new())
        }
        fn supports_loopback(&self) -> bool {
            true
        }
        fn open_capture(
            &self,
            source: &CaptureSource,
            channels: u16,
            mut on_audio: sp_audio_io::CaptureCallback,
            _on_error: ErrorCallback,
        ) -> Result<Box<dyn AudioStream>, sp_audio_io::AudioError> {
            let level = match source {
                CaptureSource::SystemLoopback(_) => 0.4f32,
                _ => 0.2,
            };
            let buf = vec![level; 480 * channels.clamp(1, 2) as usize];
            let running = Arc::new(AtomicBool::new(true));
            let flag = running.clone();
            let thread = std::thread::spawn(move || {
                // Paced by absolute deadlines, like a sound card's clock: a relative sleep
                // oversleeps a little every time (by whole percents on macOS), so two such
                // "devices" would drift apart far faster than any real pair of audio clocks.
                let start = std::time::Instant::now();
                let mut n = 0u32;
                while flag.load(Ordering::Relaxed) {
                    on_audio(&buf);
                    n += 1;
                    let due = start + Duration::from_millis(10) * n;
                    if let Some(wait) = due.checked_duration_since(std::time::Instant::now()) {
                        std::thread::sleep(wait);
                    }
                }
            });
            Ok(Box::new(Ticker {
                running,
                thread: Some(thread),
            }))
        }
        fn open_render(
            &self,
            _target: &sp_audio_io::RenderTarget,
            _channels: u16,
            _on_audio: sp_audio_io::RenderCallback,
            _on_error: ErrorCallback,
        ) -> Result<Box<dyn AudioStream>, sp_audio_io::AudioError> {
            Err(sp_audio_io::AudioError::DeviceNotFound("no output".into()))
        }
    }

    const FRAME: usize = 480;

    /// A mix ring with room for plenty of frames, and a producer standing in for the capture.
    fn mix_ring() -> (rtrb::Producer<f32>, rtrb::Consumer<f32>) {
        rtrb::RingBuffer::new(FRAME * 16)
    }

    fn push_frames(p: &mut rtrb::Producer<f32>, frames: usize, value: f32) {
        for _ in 0..frames * FRAME {
            p.push(value).unwrap();
        }
    }

    #[test]
    fn the_mix_waits_for_a_cushion_and_never_pads_a_frame() {
        let (mut p, mut c) = mix_ring();
        let mut reader = MixReader::new(1);
        let mut out = vec![1.0f32; FRAME];

        // One frame in hand is not enough to start: silence, and nothing is taken.
        push_frames(&mut p, 1, 0.5);
        reader.take(&mut c, &mut out);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(c.slots(), FRAME);

        // With the cushion, whole frames come out.
        push_frames(&mut p, 1, 0.5);
        reader.take(&mut c, &mut out);
        assert!(out.iter().all(|s| *s == 0.5));

        // A partial frame is never mixed with zeros: half a frame short plays a whole silent
        // frame and leaves the samples for later.
        let queued = c.slots();
        let _ = c.read_chunk(queued).map(|chunk| chunk.commit_all());
        for _ in 0..FRAME / 2 {
            p.push(0.5).unwrap();
        }
        reader.take(&mut c, &mut out);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(c.slots(), FRAME / 2);
    }

    #[test]
    fn a_capture_whose_callbacks_do_not_line_up_with_frames_leaves_no_gaps() {
        let (mut p, mut c) = mix_ring();
        let mut reader = MixReader::new(1);
        let mut out = vec![0.0f32; FRAME];
        // A real capture delivers the same rate in its own buffer size — 512 samples here against
        // 480-sample frames — so at the moment a frame is wanted it is often a few samples short.
        // Starting from an empty ring, as a capture does.
        const CALLBACK: usize = 512;
        let mut pushed = 0;
        let mut gaps = 0;
        // Scheduling jitter, reproducibly: now and then the capture thread runs a tick late and
        // delivers what it owes on the next one (never two late ticks running — that is a stall,
        // not jitter).
        let mut rng = 0x2545_F491_4F6C_DD1D_u64;
        let mut was_late = false;
        for tick in 0..1_000 {
            rng = rng
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let late = !was_late && (rng >> 33) % 4 == 0;
            was_late = late;
            // A callback arrives once its whole buffer has been captured, so the capture is
            // always up to one callback behind the frames the encoder has asked for.
            while !late && pushed + CALLBACK <= (tick + 1) * FRAME {
                for _ in 0..CALLBACK {
                    p.push(0.5).unwrap();
                }
                pushed += CALLBACK;
            }
            reader.take(&mut c, &mut out);
            // The first ticks build the cushion; after that no frame may contain a hole.
            if tick >= MIX_PREFILL_FRAMES * 2 && out.contains(&0.0) {
                gaps += 1;
            }
        }
        assert_eq!(gaps, 0, "{gaps} of 1000 frames had a gap");
    }

    /// Ten minutes of 10 ms frames with the microphone's clock `ppm` off the system audio's.
    /// Returns the frames that had a gap and the most the ring ever held.
    fn run_with_clock_offset(ppm: f64) -> (usize, usize) {
        let (mut p, mut c) = rtrb::RingBuffer::new(FRAME * 16);
        let mut reader = MixReader::new(1);
        let mut out = vec![0.0f32; FRAME];
        const CALLBACK: usize = 512;
        let rate = 1.0 + ppm * 1e-6;
        let (mut pushed, mut gaps, mut most) = (0usize, 0usize, 0usize);
        for tick in 0..60_000usize {
            // The microphone has captured `rate` times as much as the frames asked for so far,
            // delivered in whole callbacks.
            let captured = ((tick + 1) as f64 * FRAME as f64 * rate) as usize;
            while pushed + CALLBACK <= captured {
                for _ in 0..CALLBACK {
                    p.push(0.5).unwrap();
                }
                pushed += CALLBACK;
            }
            most = most.max(c.slots());
            reader.take(&mut c, &mut out);
            if tick >= MIX_PREFILL_FRAMES * 2 && out.contains(&0.0) {
                gaps += 1;
            }
        }
        (gaps, most)
    }

    #[test]
    fn microphone_clock_drift_is_absorbed_without_dropouts_or_skips() {
        // Real device pairs differ by tens to hundreds of ppm; ±300 ppm drains or overfills a
        // 20 ms cushion in about a minute without compensation.
        for ppm in [-300.0, 300.0] {
            let (gaps, most) = run_with_clock_offset(ppm);
            assert_eq!(gaps, 0, "{ppm} ppm: {gaps} frames had a dropout");
            assert!(
                most < FRAME * MIX_MAX_BACKLOG_FRAMES,
                "{ppm} ppm: the backlog reached {most} samples, so audio was skipped"
            );
        }
    }

    #[test]
    fn a_faster_capture_clock_cannot_drift_the_mix_behind() {
        let (mut p, mut c) = mix_ring();
        let mut reader = MixReader::new(1);
        let mut out = vec![0.0f32; FRAME];
        // Three frames arrive for every one the encode thread takes.
        for _ in 0..20 {
            push_frames(&mut p, 3, 0.5);
            reader.take(&mut c, &mut out);
            assert!(c.slots() <= FRAME * MIX_MAX_BACKLOG_FRAMES);
        }
    }

    /// Peak of the newest packet, decoded from lossless PCM; `None` before the first one.
    fn last_level(c: &Collect) -> Option<f32> {
        let packet = packets(c).pop()?;
        assert_eq!(packet.header.codec, Codec::PcmS16Le);
        Some(
            packet
                .payload
                .chunks_exact(2)
                .map(|s| f32::from(i16::from_le_bytes([s[0], s[1]])) / 32_768.0)
                .fold(0.0f32, |m, s| m.max(s.abs())),
        )
    }

    #[test]
    fn a_mixed_source_sums_both_parts_with_their_own_gains() {
        let profile = build_profile(LatencyProfile::Balanced, Quality::Lossless, 2, false);
        let group_controls = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        let group = Sender::start(
            &TwoSources,
            SenderConfig {
                profile: profile.clone(),
                application: OpusApplication::LowDelay,
                source: CaptureSource::SystemLoopback(None),
                echo: None,
                mix: Some(CaptureSource::DefaultInput),
            },
            group_controls.clone(),
            Box::new(|_| {}),
        )
        .unwrap();
        let out = Arc::new(Collect::default());
        let _sub = group.subscribe(Subscriber {
            route: 1,
            sink: out.clone(),
            dtx: true,
            controls: Arc::new(SenderControls::new(0.0, false, profile.bitrate)),
        });

        // Both parts at unity: 0.4 of system audio plus 0.2 of microphone.
        settles_at(&out, 0.6, "both parts at unity");

        // −6 dB on the system half alone: about 0.2 + 0.2.
        group_controls.system_gain_db.set(-6.0);
        settles_at(&out, 0.4, "system audio at −6 dB");

        // The microphone mute silences its half; the system audio keeps playing.
        group_controls.mix_muted.store(true, Ordering::Relaxed);
        settles_at(&out, 0.2, "microphone muted");

        group_controls.mix_muted.store(false, Ordering::Relaxed);
        group_controls.system_gain_db.set(0.0);
        group_controls.mix_gain_db.set(-20.0);
        settles_at(&out, 0.42, "microphone at −20 dB");
    }

    /// Wait until the newest packets carry `expected`, then check they keep doing so.
    ///
    /// The capture threads here are real threads, so how soon a change reaches a packet depends on
    /// the machine; a single sample after a fixed sleep fails on a slow CI runner for no reason.
    /// What must hold is that the level gets there and then stays there: a mix with gaps would
    /// keep dropping back to the system audio alone, which the second check catches.
    fn settles_at(out: &Collect, expected: f32, what: &str) {
        let close = |level: Option<f32>| level.is_some_and(|l| (l - expected).abs() < 0.02);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !close(last_level(out)) {
            assert!(
                std::time::Instant::now() < deadline,
                "{what}: level never reached {expected}, last {:?}",
                last_level(out)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // And it is where the mix sits, not a moment passing through: most samples after it stay.
        // Most rather than all, because these fake devices are ordinary threads, and a shared CI
        // machine can hold one back longer than the cushion (real capture callbacks run at
        // real-time priority). Gaps and drift are covered exactly, without threads, by the
        // `MixReader` tests above; what only this test can show is that each gain and the mute
        // reach the right half of a running stream.
        let mut held = 0;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(10));
            if close(last_level(out)) {
                held += 1;
            }
        }
        assert!(
            held >= 15,
            "{what}: only {held} of 20 samples stayed at {expected}"
        );
    }

    fn packets(c: &Collect) -> Vec<MediaPacket> {
        c.0.lock()
            .unwrap()
            .iter()
            .map(|b| MediaPacket::decode(b.clone()).unwrap())
            .collect()
    }

    #[test]
    fn one_capture_fans_out_to_every_subscriber() {
        let backend = Counting(
            NullBackend {
                recorded: None,
                capture_frequency: 440.0,
            },
            AtomicUsize::new(0),
        );
        let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 2, false);
        let group = Sender::start(
            &backend,
            SenderConfig {
                profile: profile.clone(),
                application: OpusApplication::LowDelay,
                source: CaptureSource::SystemLoopback(None),
                echo: None,
                mix: None,
            },
            Arc::new(SenderControls::new(0.0, false, profile.bitrate)),
            Box::new(|_| {}),
        )
        .unwrap();

        let (a, b) = (Arc::new(Collect::default()), Arc::new(Collect::default()));
        let a_controls = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        let b_controls = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        let sub_a = group.subscribe(Subscriber {
            route: 2,
            sink: a.clone(),
            dtx: true,
            controls: a_controls.clone(),
        });
        let sub_b = group.subscribe(Subscriber {
            route: 7,
            sink: b.clone(),
            dtx: false,
            controls: b_controls.clone(),
        });
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            backend.1.load(Ordering::Relaxed),
            1,
            "one capture for both routes"
        );
        assert_eq!(group.subscribers(), 2);

        let (pa, pb) = (packets(&a), packets(&b));
        assert!(pa.len() > 10 && pb.len() > 10);
        assert!(pa.iter().all(|p| p.header.route == 2));
        assert!(pb.iter().all(|p| p.header.route == 7));
        assert!(pa[0].header.flags.contains(MediaFlags::MARKER));
        assert!(!pa[1].header.flags.contains(MediaFlags::MARKER));
        // Same encoded audio, same timeline.
        let common = pb[0].header.seq;
        let same = pa.iter().find(|p| p.header.seq == common).unwrap();
        assert_eq!(same.payload, pb[0].payload);
        assert_eq!(same.header.sample_timestamp, pb[0].header.sample_timestamp);

        // Per-route mute: B stops receiving, A continues; unmuting marks a discontinuity.
        b_controls.muted.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(100));
        let muted_len = b.0.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(b.0.lock().unwrap().len(), muted_len);
        b_controls.muted.store(false, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(200));
        let pb = packets(&b);
        assert!(
            pb[muted_len]
                .header
                .flags
                .contains(MediaFlags::DISCONTINUITY)
        );

        // The shared encoder follows the lowest requested bitrate; unsubscribing is clean.
        a_controls.bitrate.store(48_000, Ordering::Relaxed);
        drop(sub_b);
        assert_eq!(group.subscribers(), 1);
        drop(sub_a);
        assert_eq!(group.subscribers(), 0);
        drop(group);
    }

    #[test]
    fn silence_becomes_dtx_only_for_receivers_that_understand_it() {
        let backend = NullBackend {
            recorded: None,
            capture_frequency: 0.0,
        };
        let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 1, false);
        let controls = || Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        let group = Sender::start(
            &backend,
            SenderConfig {
                profile: profile.clone(),
                application: OpusApplication::Voip,
                source: CaptureSource::DefaultInput,
                echo: None,
                mix: None,
            },
            controls(),
            Box::new(|_| {}),
        )
        .unwrap();
        let (dtx, legacy) = (Arc::new(Collect::default()), Arc::new(Collect::default()));
        let _a = group.subscribe(Subscriber {
            route: 1,
            sink: dtx.clone(),
            dtx: true,
            controls: controls(),
        });
        let _b = group.subscribe(Subscriber {
            route: 2,
            sink: legacy.clone(),
            dtx: false,
            controls: controls(),
        });
        std::thread::sleep(Duration::from_millis(1500));

        let (pd, pl) = (packets(&dtx), packets(&legacy));
        assert!(pl.len() > 100, "no DTX: every frame ({})", pl.len());
        assert!(pl.iter().all(|p| !p.header.flags.contains(MediaFlags::DTX)));
        // 200 ms of hangover audio, two DTX packets, then a keep-alive every 400 ms.
        assert!(pd.len() < 40, "silence costs a few packets ({})", pd.len());
        let silent: Vec<_> = pd
            .iter()
            .filter(|p| p.header.flags.contains(MediaFlags::DTX))
            .collect();
        assert!(silent.len() >= 3, "DTX packets: {}", silent.len());
        assert!(silent.iter().all(|p| p.payload.is_empty()));
    }
}
