//! Sender pipeline: capture → DSP → encode → media datagrams.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::Bytes;
use sp_audio_io::{AudioBackend, AudioStream, CaptureSource, ErrorCallback};
use sp_media::codec::{Encoder, OpusApplication, encoder_for};
use sp_media::dsp::{Gain, LevelMeter, NoiseSuppressor, SoftLimiter, db_to_gain};
use sp_media::samples_per_frame;
use sp_protocol::control::StreamProfile;
use sp_protocol::{Codec, MediaFlags, MediaHeader, MediaPacket};
use tracing::{debug, warn};

use super::controls::SenderControls;
use crate::EngineError;

/// Something that can carry media datagrams (a QUIC connection, or a test sink).
pub trait DatagramSink: Send + Sync + 'static {
    fn send(&self, datagram: Bytes) -> Result<(), ()>;
}

impl DatagramSink for sp_transport::SecureConnection {
    fn send(&self, datagram: Bytes) -> Result<(), ()> {
        self.send_datagram(datagram).map_err(|_| ())
    }
}

pub struct SenderConfig {
    pub route: u8,
    pub profile: StreamProfile,
    pub application: OpusApplication,
    pub source: CaptureSource,
}

pub struct Sender {
    _capture: Box<dyn AudioStream>,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub controls: Arc<SenderControls>,
}

impl Sender {
    pub fn start(
        backend: &dyn AudioBackend,
        config: SenderConfig,
        controls: Arc<SenderControls>,
        sink: Arc<dyn DatagramSink>,
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
        let thread = {
            let running = running.clone();
            let controls = controls.clone();
            let route = config.route;
            let codec = encoder.codec();
            std::thread::Builder::new()
                .name("sp-encode".into())
                .spawn(move || {
                    encode_loop(
                        &running,
                        &controls,
                        &mut consumer,
                        encoder.as_mut(),
                        sink.as_ref(),
                        route,
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
            controls,
        })
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

#[allow(clippy::too_many_arguments)]
fn encode_loop(
    running: &AtomicBool,
    controls: &SenderControls,
    input: &mut rtrb::Consumer<f32>,
    encoder: &mut dyn Encoder,
    sink: &dyn DatagramSink,
    route: u8,
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
    let mut first = true;

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

        // Live control changes.
        let bitrate = controls.bitrate.load(Ordering::Relaxed);
        if bitrate != applied_bitrate {
            encoder.set_bitrate(bitrate);
            applied_bitrate = bitrate;
        }
        let loss = controls.expected_loss_pct.load(Ordering::Relaxed);
        if loss != applied_loss {
            encoder.set_expected_loss(loss.min(100) as u8);
            applied_loss = loss;
        }
        gain.set(db_to_gain(controls.gain_db.get()));

        // DSP.
        if controls.noise_suppression.load(Ordering::Relaxed) && channels == 1 && frame % 480 == 0 {
            denoiser.get_or_insert_with(NoiseSuppressor::new).process(&mut buf);
        } else {
            denoiser = None;
        }
        gain.process(&mut buf);
        let clipped = limiter.process(&mut buf);
        meter.process(&buf);
        controls.level_db.set(meter.peak_db());
        controls.clipping.store(clipped, Ordering::Relaxed);
        if controls.muted.load(Ordering::Relaxed) {
            buf.fill(0.0);
        }

        if let Err(e) = encoder.encode(&buf, &mut packet) {
            warn!(error = %e, "encode failed");
            continue;
        }
        let mut flags = MediaFlags::empty();
        if first {
            flags = flags.with(MediaFlags::MARKER);
            first = false;
        }
        // Redundancy (Opus only): [u16 primary length][primary][previous frame].
        payload.clear();
        let redundant = controls.redundancy.load(Ordering::Relaxed)
            && codec == Codec::Opus
            && !previous.is_empty()
            && 2 + packet.len() + previous.len() <= sp_protocol::media::MAX_MEDIA_PAYLOAD;
        if redundant {
            flags = flags.with(MediaFlags::REDUNDANT);
            payload.extend_from_slice(&(packet.len() as u16).to_be_bytes());
            payload.extend_from_slice(&packet);
            payload.extend_from_slice(&previous);
        } else {
            payload.extend_from_slice(&packet);
        }
        previous.clear();
        previous.extend_from_slice(&packet);

        let media = MediaPacket {
            header: MediaHeader {
                route,
                codec,
                flags,
                sample_timestamp: timestamp,
                seq,
            },
            payload: Bytes::copy_from_slice(&payload),
        };
        match media.encode() {
            Ok(wire) => {
                let len = wire.len() as u64;
                if sink.send(wire).is_ok() {
                    controls.frames_sent.fetch_add(1, Ordering::Relaxed);
                    controls.bytes_sent.fetch_add(len, Ordering::Relaxed);
                } else {
                    controls.send_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => debug!(error = %e, "packet too large"),
        }
        timestamp += frame as u64;
        seq = seq.wrapping_add(1);
    }
}
