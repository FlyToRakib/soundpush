//! Receiver pipeline: media packets → jitter buffer → decode/PLC → drift compensation → render.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use bytes::Bytes;
use sp_audio_io::{AudioBackend, AudioStream, ErrorCallback, RenderTarget};
use sp_media::codec::{Decoder, decoder_for};
use sp_media::drift::{DriftController, FractionalResampler};
use sp_media::dsp::{Gain, LevelMeter, NoiseSuppressor, balance_and_mono};
use sp_media::jitter::{JitterBuffer, JitterConfig, PopStatus};
use sp_media::{ms_to_samples, samples_per_frame};
use sp_protocol::control::StreamProfile;
use sp_protocol::{Codec, MediaFlags, MediaPacket};

use super::controls::{EchoReference, ReceiverControls};
use crate::EngineError;

/// A packet queued from the network task to the audio thread.
pub struct InboundPacket {
    pub timestamp: u64,
    pub arrival_samples: u64,
    pub codec: Codec,
    pub discontinuity: bool,
    pub redundant: bool,
    /// DTX: the sender is silent from `timestamp` until its next audio packet (no payload).
    pub dtx: bool,
    pub payload: Bytes,
}

/// Network-side handle used by the session's datagram dispatcher.
pub struct PacketSink {
    producer: rtrb::Producer<InboundPacket>,
    epoch: Instant,
    controls: Arc<ReceiverControls>,
}

impl PacketSink {
    pub fn push(&mut self, packet: MediaPacket) {
        let arrival_samples = (self.epoch.elapsed().as_micros() as u64) * 48 / 1000;
        self.controls
            .bytes_received
            .fetch_add(packet.payload.len() as u64 + 16, Ordering::Relaxed);
        let inbound = InboundPacket {
            timestamp: packet.header.sample_timestamp,
            arrival_samples,
            codec: packet.header.codec,
            discontinuity: packet.header.flags.contains(MediaFlags::DISCONTINUITY),
            redundant: packet.header.flags.contains(MediaFlags::REDUNDANT),
            dtx: packet.header.flags.contains(MediaFlags::DTX),
            payload: packet.payload,
        };
        // A full queue means the audio thread stalled; dropping is correct.
        let _ = self.producer.push(inbound);
    }
}

pub struct Receiver {
    _render: Box<dyn AudioStream>,
    pub controls: Arc<ReceiverControls>,
}

pub struct ReceiverConfig {
    pub profile: StreamProfile,
    pub target: RenderTarget,
    /// Set when this stream goes to the device's own speakers, so microphone captures here can
    /// duck while it plays (plan §15.7).
    pub echo: Option<Arc<EchoReference>>,
}

impl Receiver {
    pub fn start(
        backend: &dyn AudioBackend,
        config: ReceiverConfig,
        controls: Arc<ReceiverControls>,
        on_error: ErrorCallback,
    ) -> Result<(Self, PacketSink), EngineError> {
        let channels = config.profile.channels.clamp(1, 2) as usize;
        let (producer, consumer) = rtrb::RingBuffer::<InboundPacket>::new(1024);
        let decoder =
            decoder_for(&config.profile).map_err(|e| EngineError::Internal(e.to_string()))?;
        let expected_codec = Codec::try_from(config.profile.codec as u8).unwrap_or(Codec::Opus);
        let mut playout = Playout::new(
            &config.profile,
            channels,
            consumer,
            decoder,
            expected_codec,
            controls.clone(),
            config.echo.clone(),
        );

        let render = backend.open_render(
            &config.target,
            channels as u16,
            Box::new(move |out: &mut [f32]| playout.fill(out)),
            on_error,
        )?;
        controls
            .device_latency_ms
            .store(render.info().latency_ms, Ordering::Relaxed);

        let sink = PacketSink {
            producer,
            epoch: Instant::now(),
            controls: controls.clone(),
        };
        Ok((
            Self {
                _render: render,
                controls,
            },
            sink,
        ))
    }
}

/// Audio-thread state. Everything here runs inside the render callback.
struct Playout {
    channels: usize,
    frame_samples: usize,
    input: rtrb::Consumer<InboundPacket>,
    decoder: Box<dyn Decoder>,
    codec: Codec,
    jitter: JitterBuffer,
    drift: DriftController,
    resampler: FractionalResampler,
    frame_buf: Vec<f32>,
    decode_buf: Vec<f32>,
    acc: Vec<f32>,
    gain: Gain,
    meter: LevelMeter,
    controls: Arc<ReceiverControls>,
    /// Noise suppression asked for by the microphone's own device; built on first use.
    denoiser: Option<NoiseSuppressor>,
    echo: Option<Arc<EchoReference>>,
    pulls: u64,
    bounds: (u32, u32, i32),
}

impl Playout {
    #[allow(clippy::too_many_arguments)]
    fn new(
        profile: &StreamProfile,
        channels: usize,
        input: rtrb::Consumer<InboundPacket>,
        decoder: Box<dyn Decoder>,
        codec: Codec,
        controls: Arc<ReceiverControls>,
        echo: Option<Arc<EchoReference>>,
    ) -> Self {
        let frame = samples_per_frame(profile.frame_us);
        let min = controls.jitter_min_ms.load(Ordering::Relaxed);
        let max = controls.jitter_max_ms.load(Ordering::Relaxed);
        Self {
            channels,
            frame_samples: frame,
            input,
            decoder,
            codec,
            jitter: JitterBuffer::new(JitterConfig {
                channels,
                frame_samples: frame,
                min_delay_ms: min,
                max_delay_ms: max,
            }),
            drift: DriftController::new(),
            resampler: FractionalResampler::new(channels),
            frame_buf: vec![0.0; frame * channels],
            decode_buf: vec![0.0; 5760 * channels],
            acc: Vec::with_capacity(4 * 5760 * channels),
            gain: Gain::new(controls.volume.get()),
            meter: LevelMeter::default(),
            controls,
            denoiser: None,
            echo,
            pulls: 0,
            bounds: (min, max, 0),
        }
    }

    fn fill(&mut self, out: &mut [f32]) {
        self.apply_bounds();
        self.drain_network();

        while self.acc.len() < out.len() {
            let status = self.jitter.pop(&mut self.frame_buf);
            let ratio = match status {
                PopStatus::Played => self
                    .drift
                    .update(self.jitter.buffered_samples(), self.jitter.target_samples()),
                PopStatus::Missing => {
                    self.decoder.conceal(&mut self.frame_buf);
                    self.drift.ratio()
                }
                PopStatus::Buffering => {
                    self.frame_buf.fill(0.0);
                    1.0
                }
                // Comfort silence while the sender is in DTX; the drift estimate holds.
                PopStatus::Silence => self.drift.ratio(),
            };
            // Noise suppression the microphone's own device asked us to run (plan §15.7). It sits
            // before the drift resampler so RNNoise always sees whole 10 ms frames.
            if self.controls.noise_suppression.load(Ordering::Relaxed)
                && self.channels == 1
                && self.frame_samples % 480 == 0
            {
                self.denoiser
                    .get_or_insert_with(NoiseSuppressor::new)
                    .process(&mut self.frame_buf);
            } else if self.denoiser.is_some() {
                self.denoiser = None;
            }
            self.resampler
                .process(&self.frame_buf, ratio, &mut self.acc);
        }
        out.copy_from_slice(&self.acc[..out.len()]);
        self.acc.drain(..out.len());

        // Output processing.
        self.gain
            .set(if self.controls.muted.load(Ordering::Relaxed) {
                0.0
            } else {
                self.controls.volume.get()
            });
        self.gain.process(out);
        if self.channels == 2 {
            balance_and_mono(
                out,
                self.controls.balance.get(),
                self.controls.mono.load(Ordering::Relaxed),
            );
        }
        self.meter.process(out);
        // Tell microphone captures on this device how loud its speakers are right now.
        if let Some(echo) = &self.echo {
            echo.set_level_db(self.meter.peak_db());
        }

        self.pulls += 1;
        if self.pulls % 25 == 0 {
            self.publish_stats();
        }
        if self.pulls % 100 == 0 {
            self.jitter.adapt();
        }
    }

    fn drain_network(&mut self) {
        while let Ok(packet) = self.input.pop() {
            self.controls
                .packets_received
                .fetch_add(1, Ordering::Relaxed);
            if packet.codec != self.codec {
                self.controls.decode_errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if packet.discontinuity {
                self.jitter.reset();
                self.drift.reset();
            }
            if packet.dtx {
                self.jitter
                    .push_silence(packet.timestamp, packet.arrival_samples);
                continue;
            }
            let bytes = &packet.payload[..];
            let (primary, redundant) = match bytes {
                [hi, lo, rest @ ..] if packet.redundant => {
                    let n = u16::from_be_bytes([*hi, *lo]) as usize;
                    if n <= rest.len() {
                        (&rest[..n], Some(&rest[n..]))
                    } else {
                        (bytes, None)
                    }
                }
                _ => (bytes, None),
            };
            // Rebuild the previous frame if it was lost and is still playable.
            if let Some(copy) = redundant.filter(|c| !c.is_empty()) {
                let prev_ts = packet.timestamp.checked_sub(self.frame_samples as u64);
                if let Some(prev_ts) = prev_ts.filter(|ts| !self.jitter.has(*ts)) {
                    if let Ok(samples) = self.decoder.decode(copy, &mut self.decode_buf) {
                        let n = samples * self.channels;
                        self.jitter.push(
                            prev_ts,
                            self.decode_buf[..n].to_vec(),
                            packet.arrival_samples,
                        );
                        self.controls
                            .packets_recovered
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            match self.decoder.decode(primary, &mut self.decode_buf) {
                Ok(samples) => {
                    let n = samples * self.channels;
                    self.jitter.push(
                        packet.timestamp,
                        self.decode_buf[..n].to_vec(),
                        packet.arrival_samples,
                    );
                }
                Err(_) => {
                    self.controls.decode_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    fn apply_bounds(&mut self) {
        let min = self.controls.jitter_min_ms.load(Ordering::Relaxed);
        let max = self.controls.jitter_max_ms.load(Ordering::Relaxed);
        let offset = self.controls.av_offset_ms.load(Ordering::Relaxed);
        if (min, max, offset) != self.bounds {
            let extra = offset.max(0) as u32;
            self.jitter.set_bounds(min + extra, max + extra);
            self.bounds = (min, max, offset);
        }
    }

    fn publish_stats(&self) {
        let s = self.jitter.stats();
        let c = &self.controls;
        c.packets_missing.store(s.missing, Ordering::Relaxed);
        c.packets_late.store(s.late, Ordering::Relaxed);
        c.underruns.store(s.underruns, Ordering::Relaxed);
        c.buffer_ms.set(s.buffered_ms as f32);
        c.target_ms.set(s.target_ms as f32);
        c.jitter_ms.set(s.jitter_ms as f32);
        c.drift_ppm.store(self.drift.ppm(), Ordering::Relaxed);
        c.level_db.set(self.meter.peak_db());
        let _ = ms_to_samples(0);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use sp_audio_io::null::NullBackend;
    use sp_audio_io::{CaptureSource, RenderTarget};
    use sp_media::codec::OpusApplication;
    use sp_media::profile::{LatencyProfile, Quality, build_profile};

    use super::*;
    use crate::pipeline::controls::SenderControls;
    use crate::pipeline::sender::{
        DatagramSink, SendFailed, Sender, SenderConfig, Subscriber, Subscription,
    };

    fn start_sender(
        backend: &NullBackend,
        profile: StreamProfile,
        controls: Arc<SenderControls>,
        sink: Arc<dyn DatagramSink>,
    ) -> (Sender, Subscription) {
        let sender = Sender::start(
            backend,
            SenderConfig {
                profile: profile.clone(),
                application: OpusApplication::LowDelay,
                source: CaptureSource::DefaultInput,
                echo: None,
            },
            Arc::new(SenderControls::new(0.0, false, profile.bitrate)),
            Box::new(|_| {}),
        )
        .unwrap();
        let subscription = sender.subscribe(Subscriber {
            route: 1,
            sink,
            dtx: true,
            controls,
        });
        (sender, subscription)
    }

    /// Loops sender datagrams straight into a receiver's packet sink, optionally dropping every Nth.
    struct Loopback(Mutex<Option<PacketSink>>, u64, std::sync::atomic::AtomicU64);

    impl DatagramSink for Loopback {
        fn send(&self, datagram: Bytes) -> Result<(), SendFailed> {
            let n = self.2.fetch_add(1, Ordering::Relaxed);
            if self.1 > 0 && n % self.1 == self.1 - 1 {
                return Ok(()); // simulated loss
            }
            let packet = MediaPacket::decode(datagram).map_err(|_| SendFailed)?;
            if let Ok(mut guard) = self.0.lock() {
                if let Some(sink) = guard.as_mut() {
                    sink.push(packet);
                }
            }
            Ok(())
        }
    }

    #[test]
    fn redundancy_recovers_single_losses() {
        let backend = NullBackend {
            recorded: None,
            capture_frequency: 440.0,
        };
        let profile = build_profile(LatencyProfile::Stable, Quality::Auto, 1, true);
        let rx_controls = Arc::new(ReceiverControls::new(
            1.0,
            profile.jitter_min_ms,
            profile.jitter_max_ms,
        ));
        let (_receiver, sink) = Receiver::start(
            &backend,
            ReceiverConfig {
                profile: profile.clone(),
                target: RenderTarget::DefaultOutput,
                echo: None,
            },
            rx_controls.clone(),
            Box::new(|_| {}),
        )
        .unwrap();
        // Drop every 5th packet.
        let loopback = Arc::new(Loopback(
            Mutex::new(Some(sink)),
            5,
            std::sync::atomic::AtomicU64::new(0),
        ));
        let tx_controls = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        tx_controls.redundancy.store(true, Ordering::Relaxed);
        let _sender = start_sender(&backend, profile, tx_controls, loopback);

        std::thread::sleep(Duration::from_millis(2000));
        let recovered = rx_controls.packets_recovered.load(Ordering::Relaxed);
        assert!(
            recovered >= 10,
            "expected recovered frames, got {recovered}"
        );
    }

    #[test]
    fn end_to_end_sine_through_opus_pipeline() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let backend = NullBackend {
            recorded: Some(recorded.clone()),
            capture_frequency: 440.0,
        };
        let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 1, false);

        let rx_controls = Arc::new(ReceiverControls::new(
            1.0,
            profile.jitter_min_ms,
            profile.jitter_max_ms,
        ));
        let (_receiver, sink) = Receiver::start(
            &backend,
            ReceiverConfig {
                profile: profile.clone(),
                target: RenderTarget::DefaultOutput,
                echo: None,
            },
            rx_controls.clone(),
            Box::new(|_| {}),
        )
        .unwrap();

        let loopback = Arc::new(Loopback(
            Mutex::new(Some(sink)),
            0,
            std::sync::atomic::AtomicU64::new(0),
        ));
        let tx_controls = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        let _sender = start_sender(&backend, profile, tx_controls.clone(), loopback);

        std::thread::sleep(Duration::from_millis(1500));

        assert!(tx_controls.frames_sent.load(Ordering::Relaxed) > 100);
        assert!(rx_controls.packets_received.load(Ordering::Relaxed) > 100);
        let audio = recorded.lock().unwrap().clone();
        let tail = &audio[audio.len() / 2..];
        let peak = tail.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.1, "received audio peak {peak}");
        assert!(rx_controls.underruns.load(Ordering::Relaxed) <= 2);
    }

    /// "Noise suppression → on the other device" (plan §15.7): the playout denoises the stream it
    /// was told to, and leaves it alone otherwise. Driven directly, so it is deterministic.
    #[test]
    fn receiver_side_noise_suppression_quietens_hiss() {
        let profile = build_profile(LatencyProfile::Balanced, Quality::Lossless, 1, false);
        let frame = sp_media::samples_per_frame(profile.frame_us);
        // Six seconds of quiet hiss as PCM frames; the same audio feeds both runs.
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut noise = Vec::new();
        for _ in 0..(frame * 600) {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            noise.push(((seed >> 40) as i16) / 32);
        }

        let rms_with = |denoise: bool| -> f32 {
            let (mut producer, consumer) = rtrb::RingBuffer::<InboundPacket>::new(512);
            let controls = Arc::new(ReceiverControls::new(
                1.0,
                profile.jitter_min_ms,
                profile.jitter_max_ms,
            ));
            controls.noise_suppression.store(denoise, Ordering::Relaxed);
            let mut playout = Playout::new(
                &profile,
                1,
                consumer,
                decoder_for(&profile).unwrap(),
                Codec::PcmS16Le,
                controls,
                None,
            );
            for (n, block) in noise.chunks_exact(frame).enumerate() {
                let mut payload = Vec::with_capacity(block.len() * 2);
                for s in block {
                    payload.extend_from_slice(&s.to_le_bytes());
                }
                let at = (n * frame) as u64;
                let _ = producer.push(InboundPacket {
                    timestamp: at,
                    arrival_samples: at,
                    codec: Codec::PcmS16Le,
                    discontinuity: false,
                    redundant: false,
                    dtx: false,
                    payload: Bytes::from(payload),
                });
            }
            let mut played = Vec::new();
            let mut out = vec![0.0f32; frame];
            for _ in 0..560 {
                playout.fill(&mut out);
                played.extend_from_slice(&out);
            }
            // The last part, after the jitter buffer filled and RNNoise settled.
            let tail = &played[played.len() * 3 / 4..];
            (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
        };

        let plain = rms_with(false);
        let denoised = rms_with(true);
        assert!(
            plain > 0.001,
            "the plain playout passes audio through: {plain}"
        );
        assert!(
            denoised < plain / 5.0,
            "the receiver should suppress hiss: {denoised} vs {plain}"
        );
    }

    #[test]
    fn dtx_silence_is_cheap_and_does_not_underrun() {
        let backend = NullBackend {
            recorded: None,
            capture_frequency: 0.0,
        };
        let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 1, false);
        let rx_controls = Arc::new(ReceiverControls::new(
            1.0,
            profile.jitter_min_ms,
            profile.jitter_max_ms,
        ));
        let (_receiver, sink) = Receiver::start(
            &backend,
            ReceiverConfig {
                profile: profile.clone(),
                target: RenderTarget::DefaultOutput,
                echo: None,
            },
            rx_controls.clone(),
            Box::new(|_| {}),
        )
        .unwrap();
        let loopback = Arc::new(Loopback(
            Mutex::new(Some(sink)),
            0,
            std::sync::atomic::AtomicU64::new(0),
        ));
        let tx_controls = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        let _sender = start_sender(&backend, profile, tx_controls, loopback.clone());

        std::thread::sleep(Duration::from_millis(2000));
        let sent = loopback.2.load(Ordering::Relaxed);
        assert!(sent < 50, "datagrams during 2 s of silence: {sent}");
        // Without DTX handling every silent stretch would conceal, underrun and rebuffer.
        let underruns = rx_controls.underruns.load(Ordering::Relaxed);
        assert!(underruns <= 1, "underruns during silence: {underruns}");
        assert_eq!(rx_controls.decode_errors.load(Ordering::Relaxed), 0);
    }
}
