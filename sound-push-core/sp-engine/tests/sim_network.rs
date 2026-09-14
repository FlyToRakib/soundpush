//! Simulated network between a real sender and receiver pipeline (plan §29.1 "Simulation"):
//! loss, jitter (and the reordering it causes), duplication and blackouts are injected at the
//! transport boundary, where media datagrams leave the encoder group and reach the receiver.
//! Everything else is the production pipeline: capture, DSP, Opus, redundancy, jitter buffer,
//! concealment, drift compensation and render.
//!
//! The impairments are drawn from a seeded RNG, so a failure reproduces. The audio devices tick in
//! real time, so each scenario runs for a few seconds; the tests run in parallel.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers fail the test on purpose

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use sp_engine::pipeline::receiver::{PacketSink, Receiver, ReceiverConfig};
use sp_engine::pipeline::sender::{DatagramSink, SendFailed, Sender, SenderConfig, Subscriber};
use sp_engine::pipeline::{ReceiverControls, SenderControls};
use sp_engine::sp_audio_io::null::NullBackend;
use sp_engine::sp_audio_io::{CaptureSource, RenderTarget};
use sp_media::codec::OpusApplication;
use sp_media::profile::{LatencyProfile, Quality, build_profile};
use sp_protocol::MediaPacket;
use sp_protocol::control::StreamProfile;

/// What the simulated path does to each datagram.
#[derive(Debug, Clone, Copy, Default)]
struct Impairments {
    /// Probability that a datagram is lost.
    loss: f64,
    /// Fixed one-way delay.
    delay: Duration,
    /// Extra delay drawn uniformly from `0..=jitter` per datagram; datagrams overtake each other.
    jitter: Duration,
    /// Probability that a datagram is delivered twice.
    duplicate: f64,
    /// Nothing gets through between these offsets from the start of the link.
    blackout: Option<(Duration, Duration)>,
}

#[derive(Default)]
struct Stats {
    sent: AtomicU64,
    dropped: AtomicU64,
    duplicated: AtomicU64,
    delivered: AtomicU64,
}

type Queue = BinaryHeap<Reverse<(Instant, u64, Bytes)>>;

/// A one-way datagram link with impairments, delivering into a receiver's packet sink.
struct SimLink {
    impairments: Impairments,
    start: Instant,
    rng: Mutex<StdRng>,
    queue: Arc<(Mutex<Queue>, Condvar)>,
    order: AtomicU64,
    stats: Arc<Stats>,
    running: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl SimLink {
    fn new(impairments: Impairments, seed: u64, mut sink: PacketSink) -> Arc<Self> {
        let queue: Arc<(Mutex<Queue>, Condvar)> = Arc::default();
        let stats = Arc::new(Stats::default());
        let running = Arc::new(AtomicBool::new(true));
        let thread = std::thread::spawn({
            let (queue, stats, running) = (queue.clone(), stats.clone(), running.clone());
            move || {
                let (lock, wake) = &*queue;
                let mut q = lock.lock().unwrap();
                while running.load(Ordering::Relaxed) {
                    let now = Instant::now();
                    match q.peek() {
                        Some(Reverse((due, _, _))) if *due <= now => {
                            let Some(Reverse((_, _, datagram))) = q.pop() else {
                                continue;
                            };
                            if let Ok(packet) = MediaPacket::decode(datagram) {
                                sink.push(packet);
                                stats.delivered.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Some(Reverse((due, _, _))) => {
                            let wait = *due - now;
                            q = wake.wait_timeout(q, wait).unwrap().0;
                        }
                        None => q = wake.wait_timeout(q, Duration::from_millis(20)).unwrap().0,
                    }
                }
            }
        });
        Arc::new(Self {
            impairments,
            start: Instant::now(),
            rng: Mutex::new(StdRng::seed_from_u64(seed)),
            queue,
            order: AtomicU64::new(0),
            stats,
            running,
            thread: Mutex::new(Some(thread)),
        })
    }

    fn schedule(&self, datagram: Bytes, due: Instant) {
        let (lock, wake) = &*self.queue;
        let order = self.order.fetch_add(1, Ordering::Relaxed);
        lock.lock().unwrap().push(Reverse((due, order, datagram)));
        wake.notify_one();
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.queue.1.notify_all();
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}

impl DatagramSink for SimLink {
    fn send(&self, datagram: Bytes) -> Result<(), SendFailed> {
        let now = Instant::now();
        let i = self.impairments;
        self.stats.sent.fetch_add(1, Ordering::Relaxed);
        let since = now.duration_since(self.start);
        if i.blackout
            .is_some_and(|(from, to)| since >= from && since < to)
        {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        let mut rng = self.rng.lock().unwrap();
        if rng.gen_bool(i.loss.clamp(0.0, 1.0)) {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        let copies = if rng.gen_bool(i.duplicate.clamp(0.0, 1.0)) {
            self.stats.duplicated.fetch_add(1, Ordering::Relaxed);
            2
        } else {
            1
        };
        for _ in 0..copies {
            let extra = rng.gen_range(0..=i.jitter.as_micros() as u64);
            self.schedule(
                datagram.clone(),
                now + i.delay + Duration::from_micros(extra),
            );
        }
        Ok(())
    }
}

/// A sender and receiver joined by a [`SimLink`].
struct Scenario {
    link: Arc<SimLink>,
    rx: Arc<ReceiverControls>,
    recorded: Arc<Mutex<Vec<f32>>>,
    // Dropped in declaration order: the sender stops before the link and receiver.
    _subscription: sp_engine::pipeline::sender::Subscription,
    _sender: Sender,
    _receiver: Receiver,
}

impl Scenario {
    fn start(
        profile: StreamProfile,
        impairments: Impairments,
        redundancy: bool,
        seed: u64,
    ) -> Self {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let rx_backend = NullBackend {
            recorded: Some(recorded.clone()),
            capture_frequency: 0.0,
        };
        let tx_backend = NullBackend {
            recorded: None,
            capture_frequency: 440.0,
        };
        let rx = Arc::new(ReceiverControls::new(
            1.0,
            profile.jitter_min_ms,
            profile.jitter_max_ms,
        ));
        let (receiver, sink) = Receiver::start(
            &rx_backend,
            ReceiverConfig {
                profile: profile.clone(),
                target: RenderTarget::DefaultOutput,
            },
            rx.clone(),
            Box::new(|_| {}),
        )
        .expect("receiver starts");
        let link = SimLink::new(impairments, seed, sink);
        let sender = Sender::start(
            &tx_backend,
            SenderConfig {
                profile: profile.clone(),
                application: OpusApplication::LowDelay,
                source: CaptureSource::SystemLoopback(None),
            },
            Arc::new(SenderControls::new(0.0, false, profile.bitrate)),
            Box::new(|_| {}),
        )
        .expect("sender starts");
        let tx = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        tx.redundancy.store(redundancy, Ordering::Relaxed);
        let subscription = sender.subscribe(Subscriber {
            route: 1,
            sink: link.clone(),
            dtx: true,
            controls: tx,
        });
        Self {
            link,
            rx,
            recorded,
            _subscription: subscription,
            _sender: sender,
            _receiver: receiver,
        }
    }

    fn run_for(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    /// Peak level of the last `ms` of rendered audio.
    fn recent_peak(&self, ms: usize) -> f32 {
        let audio = self.recorded.lock().unwrap();
        let n = 48 * ms;
        audio[audio.len().saturating_sub(n)..]
            .iter()
            .fold(0f32, |m, s| m.max(s.abs()))
    }

    fn underruns(&self) -> u64 {
        self.rx.underruns.load(Ordering::Relaxed)
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        self.link.stop();
    }
}

#[test]
fn redundancy_rides_out_random_loss() {
    let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 2, true);
    let s = Scenario::start(
        profile,
        Impairments {
            loss: 0.08,
            delay: Duration::from_millis(3),
            jitter: Duration::from_millis(4),
            ..Impairments::default()
        },
        true,
        1,
    );
    s.run_for(Duration::from_secs(3));
    let dropped = s.link.stats.dropped.load(Ordering::Relaxed);
    let recovered = s.rx.packets_recovered.load(Ordering::Relaxed);
    assert!(dropped > 10, "the link dropped packets ({dropped})");
    assert!(
        recovered * 2 >= dropped,
        "most single losses are rebuilt from the redundant copy: {recovered} of {dropped}"
    );
    assert!(s.recent_peak(500) > 0.1, "audio keeps playing");
    assert!(s.underruns() <= 3, "underruns {}", s.underruns());
    assert_eq!(s.rx.decode_errors.load(Ordering::Relaxed), 0);
}

#[test]
fn jitter_and_reordering_are_absorbed_by_the_buffer() {
    // Up to 40 ms of jitter at 10 ms frames reorders packets constantly.
    let profile = build_profile(LatencyProfile::Stable, Quality::Auto, 2, false);
    let s = Scenario::start(
        profile,
        Impairments {
            delay: Duration::from_millis(5),
            jitter: Duration::from_millis(40),
            ..Impairments::default()
        },
        false,
        2,
    );
    s.run_for(Duration::from_secs(4));
    assert!(s.recent_peak(500) > 0.1, "audio keeps playing");
    assert!(
        s.underruns() <= 2,
        "the adaptive buffer grows instead of underrunning: {}",
        s.underruns()
    );
    let target = s.rx.target_ms.get();
    assert!(
        (20.0..=250.0).contains(&target),
        "target within the Stable bounds: {target}"
    );
}

#[test]
fn duplicated_packets_are_harmless() {
    let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 2, false);
    let s = Scenario::start(
        profile,
        Impairments {
            duplicate: 0.25,
            jitter: Duration::from_millis(8),
            ..Impairments::default()
        },
        false,
        3,
    );
    s.run_for(Duration::from_secs(2));
    assert!(s.link.stats.duplicated.load(Ordering::Relaxed) > 20);
    assert!(s.recent_peak(500) > 0.1);
    assert!(s.underruns() <= 2, "underruns {}", s.underruns());
    assert_eq!(s.rx.decode_errors.load(Ordering::Relaxed), 0);
}

#[test]
fn audio_returns_after_a_blackout() {
    let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 2, false);
    let s = Scenario::start(
        profile,
        Impairments {
            delay: Duration::from_millis(2),
            jitter: Duration::from_millis(2),
            blackout: Some((Duration::from_millis(1000), Duration::from_millis(1800))),
            ..Impairments::default()
        },
        false,
        4,
    );
    std::thread::sleep(Duration::from_millis(1500));
    // Well inside the blackout: concealment has faded to silence.
    assert!(
        s.recent_peak(200) < 0.05,
        "silence during the blackout: {}",
        s.recent_peak(200)
    );
    let during = s.rx.packets_received.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(1700));
    assert!(s.rx.packets_received.load(Ordering::Relaxed) > during + 50);
    assert!(
        s.recent_peak(500) > 0.1,
        "audio resumes within a second of the path coming back"
    );
    // A 800 ms outage is one rebuffer, not a storm.
    assert!(s.underruns() <= 3, "underruns {}", s.underruns());
    assert!(s.rx.target_ms.get() <= 80.0, "target stays within Balanced");
}
