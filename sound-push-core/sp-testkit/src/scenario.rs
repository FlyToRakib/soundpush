//! A real sender and receiver pipeline joined by a simulated path.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use sp_engine::pipeline::receiver::{Receiver, ReceiverConfig};
use sp_engine::pipeline::sender::{DatagramSink, SendFailed, Sender, SenderConfig, Subscriber};
use sp_engine::pipeline::{ReceiverControls, SenderControls};
use sp_engine::sp_audio_io::null::NullBackend;
use sp_engine::sp_audio_io::{AudioBackend, CaptureSource, RenderTarget};
use sp_media::codec::OpusApplication;
use sp_protocol::MediaPacket;
use sp_protocol::control::StreamProfile;

use crate::audio::{Marks, SignalBackend, peak};
use crate::net::{Impairments, SimLink};

/// What the sender's capture produces.
pub enum Source {
    /// A continuous sine at this frequency, as the null backend generates it.
    Tone(f32),
    /// One period of a generated signal, looped (see [`crate::audio::chirp_period`]).
    Signal(Vec<f32>),
}

/// The subscriber hands datagrams to the link; the link hands packets to the receiver.
struct LinkSink(Arc<SimLink>);

impl DatagramSink for LinkSink {
    fn send(&self, datagram: Bytes) -> Result<(), SendFailed> {
        self.0.send(datagram);
        Ok(())
    }
}

/// A sender and receiver joined by a [`SimLink`].
pub struct Scenario {
    link: Arc<SimLink>,
    rx: Arc<ReceiverControls>,
    recorded: Arc<Mutex<Vec<f32>>>,
    marks: Option<Arc<Marks>>,
    channels: usize,
    // Dropped in declaration order: the sender stops before the link and receiver.
    _subscription: sp_engine::pipeline::sender::Subscription,
    _sender: Sender,
    _receiver: Receiver,
}

impl Scenario {
    /// A sine through the pipeline, which is all a loss or jitter test needs.
    pub fn tone(
        profile: StreamProfile,
        impairments: Impairments,
        redundancy: bool,
        seed: u64,
    ) -> Self {
        Self::start(profile, impairments, redundancy, seed, Source::Tone(440.0))
    }

    pub fn start(
        profile: StreamProfile,
        impairments: Impairments,
        redundancy: bool,
        seed: u64,
        source: Source,
    ) -> Self {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let mut marks = None;
        let (rx_backend, tx_backend): (Box<dyn AudioBackend>, Box<dyn AudioBackend>) = match source
        {
            Source::Tone(hz) => (
                Box::new(NullBackend {
                    recorded: Some(recorded.clone()),
                    capture_frequency: 0.0,
                }),
                Box::new(NullBackend {
                    recorded: None,
                    capture_frequency: hz,
                }),
            ),
            Source::Signal(signal) => {
                // One backend serves both ends: the sender's capture loops the signal, the
                // receiver's render is recorded, and both timestamp their first buffer.
                let backend = SignalBackend {
                    recorded: recorded.clone(),
                    ..SignalBackend::new(signal)
                };
                marks = Some(backend.marks.clone());
                (Box::new(backend.clone()), Box::new(backend))
            }
        };
        let rx = Arc::new(ReceiverControls::new(
            1.0,
            profile.jitter_min_ms,
            profile.jitter_max_ms,
        ));
        let (receiver, mut sink) = Receiver::start(
            rx_backend.as_ref(),
            ReceiverConfig {
                profile: profile.clone(),
                target: RenderTarget::DefaultOutput,
                echo: None,
            },
            rx.clone(),
            Box::new(|_| {}),
        )
        .expect("receiver starts");
        let link = SimLink::new(impairments, seed, move |datagram| {
            if let Ok(packet) = MediaPacket::decode(datagram) {
                sink.push(packet);
            }
        });
        let sender = Sender::start(
            tx_backend.as_ref(),
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
        .expect("sender starts");
        let tx = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        tx.redundancy.store(redundancy, Ordering::Relaxed);
        let subscription = sender.subscribe(Subscriber {
            route: 1,
            sink: Arc::new(LinkSink(link.clone())),
            dtx: true,
            controls: tx,
        });
        Self {
            link,
            rx,
            recorded,
            marks,
            channels: profile.channels.clamp(1, 2) as usize,
            _subscription: subscription,
            _sender: sender,
            _receiver: receiver,
        }
    }

    /// Let the scenario run in real time; the fake devices tick at 10 ms like real ones.
    pub fn run_for(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    /// Peak level of the last `ms` of rendered audio.
    pub fn recent_peak(&self, ms: usize) -> f32 {
        let audio = self.recorded.lock().unwrap();
        let n = 48 * ms * self.channels;
        peak(&audio[audio.len().saturating_sub(n)..])
    }

    /// Everything rendered so far, interleaved.
    pub fn recorded(&self) -> Vec<f32> {
        self.recorded.lock().unwrap().clone()
    }

    /// Channels on the wire, and in [`Scenario::recorded`].
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// When each fake stream started, for [`Source::Signal`] scenarios.
    pub fn marks(&self) -> Option<&Marks> {
        self.marks.as_deref()
    }

    /// The receiver's live controls and counters (underruns, recovered packets, buffer target).
    pub fn receiver(&self) -> &ReceiverControls {
        &self.rx
    }

    /// What the simulated path did with the datagrams.
    pub fn link(&self) -> &SimLink {
        &self.link
    }

    pub fn underruns(&self) -> u64 {
        self.rx.underruns.load(Ordering::Relaxed)
    }

    /// Wall-clock start of the recording, once the render stream has produced a buffer.
    pub fn render_started(&self) -> Option<Instant> {
        self.marks.as_ref()?.render_started()
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        self.link.stop();
    }
}
