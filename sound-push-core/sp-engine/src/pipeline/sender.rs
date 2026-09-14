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
use sp_media::dsp::{Gain, LevelMeter, NoiseSuppressor, SoftLimiter, db_to_gain};
use sp_media::samples_per_frame;
use sp_protocol::control::StreamProfile;
use sp_protocol::{Codec, MediaFlags, MediaHeader, MediaPacket};
use tracing::{debug, warn};

use super::controls::SenderControls;
use crate::EngineError;

/// Something that can carry media datagrams (a connection, or a test sink).
pub trait DatagramSink: Send + Sync + 'static {
    fn send(&self, datagram: Bytes) -> Result<(), ()>;
}

impl DatagramSink for sp_transport::SecureConnection {
    fn send(&self, datagram: Bytes) -> Result<(), ()> {
        self.send_datagram(datagram).map_err(|_| ())
    }
}

pub struct SenderConfig {
    pub profile: StreamProfile,
    pub application: OpusApplication,
    pub source: CaptureSource,
}

/// One route receiving a group's packets.
pub struct Subscriber {
    pub route: u8,
    pub sink: Arc<dyn DatagramSink>,
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
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    fanout: Arc<Fanout>,
    /// Group-wide controls: `gain_db`, `noise_suppression`, `muted` (microphone mute),
    /// `level_db`, `clipping`, `capture_overruns`.
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
        let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(48_000 * channels);
        let overruns = controls.clone();
        let capture = backend.open_capture(
            &config.source,
            channels as u16,
            Box::new(move |samples: &[f32]| {
                let n = samples.len().min(producer.slots());
                if n < samples.len() {
                    overruns.capture_overruns.fetch_add(1, Ordering::Relaxed);
                }
                if let Ok(mut chunk) = producer.write_chunk_uninit(n) {
                    let (a, b) = chunk.as_mut_slices();
                    for (dst, src) in a.iter_mut().chain(b.iter_mut()).zip(samples.iter()) {
                        dst.write(*src);
                    }
                    // SAFETY: all `n` slots were initialized just above.
                    unsafe { chunk.commit_all() };
                }
            }),
            on_error,
        )?;

        let running = Arc::new(AtomicBool::new(true));
        let fanout = Arc::new(Fanout::default());
        let thread = {
            let running = running.clone();
            let controls = controls.clone();
            let fanout = fanout.clone();
            let codec = encoder.codec();
            std::thread::Builder::new()
                .name("sp-encode".into())
                .spawn(move || {
                    encode_loop(
                        &running,
                        &controls,
                        &fanout,
                        &mut consumer,
                        encoder.as_mut(),
                        codec,
                        channels,
                        frame,
                    )
                })
                .map_err(|e| EngineError::Internal(e.to_string()))?
        };

        Ok(Self {
            _capture: capture,
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

/// The encode thread's copy of a subscriber.
struct Target {
    id: u64,
    route: u8,
    sink: Arc<dyn DatagramSink>,
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
            controls: e.subscriber.controls.clone(),
            restart,
            first,
        });
    }
    *targets = next;
}

#[allow(clippy::too_many_arguments)]
fn encode_loop(
    running: &AtomicBool,
    controls: &SenderControls,
    fanout: &Fanout,
    input: &mut rtrb::Consumer<f32>,
    encoder: &mut dyn Encoder,
    codec: Codec,
    channels: usize,
    frame: usize,
) {
    let mut buf = vec![0.0f32; frame * channels];
    let mut packet = Vec::with_capacity(1500);
    let mut previous: Vec<u8> = Vec::with_capacity(1500);
    let mut payload: Vec<u8> = Vec::with_capacity(1500);
    let mut gain = Gain::new(db_to_gain(controls.gain_db.get()));
    let limiter = SoftLimiter::default();
    let mut meter = LevelMeter::default();
    let mut denoiser: Option<NoiseSuppressor> = None;
    let mut applied_bitrate = 0u32;
    let mut applied_loss = u32::MAX;
    let mut timestamp: u64 = 0;
    let mut seq: u32 = 0;
    let mut targets: Vec<Target> = Vec::new();
    let mut generation = u64::MAX;

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

        // DSP.
        if controls.noise_suppression.load(Ordering::Relaxed) && channels == 1 && frame % 480 == 0 {
            denoiser
                .get_or_insert_with(NoiseSuppressor::new)
                .process(&mut buf);
        } else {
            denoiser = None;
        }
        gain.process(&mut buf);
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
        if targets.is_empty() {
            previous.clear();
            continue;
        }

        if let Err(e) = encoder.encode(&buf, &mut packet) {
            warn!(error = %e, "encode failed");
            continue;
        }

        // Wire bytes are built once per variant (with and without redundancy) and copied per
        // receiver with its route id and flags patched in.
        let mut plain: Option<Bytes> = None;
        let mut redundant: Option<Bytes> = None;
        let can_add_redundancy = codec == Codec::Opus
            && !previous.is_empty()
            && 2 + packet.len() + previous.len() <= sp_protocol::media::MAX_MEDIA_PAYLOAD;
        for t in &mut targets {
            t.controls.level_db.set(level);
            if t.controls.muted.load(Ordering::Relaxed) {
                // Nothing is sent to a muted receiver; it resynchronizes when unmuted.
                t.restart = true;
                continue;
            }
            let with_redundancy =
                can_add_redundancy && t.controls.redundancy.load(Ordering::Relaxed);
            let slot = if with_redundancy {
                &mut redundant
            } else {
                &mut plain
            };
            if slot.is_none() {
                payload.clear();
                let mut flags = MediaFlags::empty();
                if with_redundancy {
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
        previous.extend_from_slice(&packet);
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
        fn send(&self, datagram: Bytes) -> Result<(), ()> {
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
            controls: a_controls.clone(),
        });
        let sub_b = group.subscribe(Subscriber {
            route: 7,
            sink: b.clone(),
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
}
