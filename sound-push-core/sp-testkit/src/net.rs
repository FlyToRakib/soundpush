//! A simulated one-way datagram path (plan §29.1 "Simulation").
//!
//! Impairments are injected where media datagrams leave the encoder group and reach the receiver,
//! so everything above and below the transport is the production pipeline. The same profiles drive
//! `tools/netsim`, which applies them to a real interface with `tc netem` (Linux) or clumsy
//! (Windows), so a simulated run and a real run are impaired the same way.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// What the simulated path does to each datagram.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Impairments {
    /// Probability that a datagram is lost.
    pub loss: f64,
    /// Fixed one-way delay.
    pub delay: Duration,
    /// Extra delay drawn uniformly from `0..=jitter` per datagram; datagrams overtake each other.
    pub jitter: Duration,
    /// Probability that a datagram is delivered twice.
    pub duplicate: f64,
    /// Nothing gets through between these offsets from the start of the link.
    pub blackout: Option<(Duration, Duration)>,
}

/// The named profiles, in the order `netsim list` prints them.
pub const PROFILES: &[(&str, &str)] = &[
    (
        "perfect",
        "No impairment: the baseline every other profile is compared against",
    ),
    (
        "wifi-5ghz",
        "A quiet 5 GHz link: a few ms of delay, almost no loss",
    ),
    (
        "wifi-24ghz",
        "A busy 2.4 GHz link: more jitter, occasional loss",
    ),
    (
        "congested",
        "A saturated link: heavy jitter, 8 % loss, the odd duplicate",
    ),
    (
        "hotspot",
        "Phone hotspot or USB tethering: steady delay with bursts of jitter",
    ),
    (
        "lossy",
        "20 % loss, the worst case redundancy and concealment must survive",
    ),
    (
        "blackout",
        "A clean link that goes away entirely from 5 s to 6 s",
    ),
];

impl Impairments {
    /// A profile by name, as `netsim` and `latency-probe` accept it. See [`PROFILES`].
    pub fn named(name: &str) -> Option<Self> {
        let ms = Duration::from_millis;
        Some(match name {
            "perfect" => Self::default(),
            "wifi-5ghz" => Self {
                loss: 0.001,
                delay: ms(2),
                jitter: ms(3),
                ..Self::default()
            },
            "wifi-24ghz" => Self {
                loss: 0.01,
                delay: ms(6),
                jitter: ms(15),
                ..Self::default()
            },
            "congested" => Self {
                loss: 0.08,
                delay: ms(20),
                jitter: ms(60),
                duplicate: 0.02,
                ..Self::default()
            },
            "hotspot" => Self {
                loss: 0.02,
                delay: ms(12),
                jitter: ms(40),
                ..Self::default()
            },
            "lossy" => Self {
                loss: 0.20,
                delay: ms(5),
                jitter: ms(10),
                ..Self::default()
            },
            "blackout" => Self {
                delay: ms(2),
                jitter: ms(2),
                blackout: Some((Duration::from_secs(5), Duration::from_secs(6))),
                ..Self::default()
            },
            _ => return None,
        })
    }

    /// `tc qdisc … netem` arguments for the same path on Linux.
    ///
    /// netem draws its delay from `mean ± variation`, while the simulator adds `0..=jitter`, so the
    /// mean carries half the jitter. Blackouts are a separate `tc` call and are not included here.
    pub fn netem_args(&self) -> Vec<String> {
        let mut args = vec!["netem".to_string()];
        let half = self.jitter / 2;
        if !self.delay.is_zero() || !self.jitter.is_zero() {
            args.push("delay".into());
            args.push(format!("{}ms", millis(self.delay + half)));
            if !half.is_zero() {
                args.push(format!("{}ms", millis(half)));
                args.push("distribution".into());
                args.push("uniform".into());
            }
        }
        if self.loss > 0.0 {
            args.push("loss".into());
            args.push(format!("{}%", percent(self.loss)));
        }
        if self.duplicate > 0.0 {
            args.push("duplicate".into());
            args.push(format!("{}%", percent(self.duplicate)));
        }
        args
    }

    /// clumsy (Windows) command-line options for the same path.
    ///
    /// clumsy's lag is a fixed delay, so the mean of the simulated delay is used and its jitter
    /// window is left to the link itself.
    pub fn clumsy_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let mean = self.delay + self.jitter / 2;
        if !mean.is_zero() {
            args.push("--lag".into());
            args.push("on".into());
            args.push("--lag-time".into());
            args.push(millis(mean).to_string());
        }
        if self.loss > 0.0 {
            args.push("--drop".into());
            args.push("on".into());
            args.push("--drop-chance".into());
            args.push(percent(self.loss).to_string());
        }
        if self.duplicate > 0.0 {
            args.push("--duplicate".into());
            args.push("on".into());
            args.push("--duplicate-chance".into());
            args.push(percent(self.duplicate).to_string());
        }
        args
    }
}

fn millis(d: Duration) -> u64 {
    d.as_millis() as u64
}

fn percent(p: f64) -> f64 {
    (p * 1000.0).round() / 10.0
}

/// What the link did with the datagrams handed to it.
#[derive(Default)]
pub struct LinkStats {
    sent: AtomicU64,
    dropped: AtomicU64,
    duplicated: AtomicU64,
    delivered: AtomicU64,
}

impl LinkStats {
    /// Datagrams handed to the link.
    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }

    /// Datagrams the link threw away (loss or blackout).
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Datagrams the link delivered twice.
    pub fn duplicated(&self) -> u64 {
        self.duplicated.load(Ordering::Relaxed)
    }

    /// Deliveries at the far end, duplicates included.
    pub fn delivered(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed)
    }
}

type Queue = BinaryHeap<Reverse<(Instant, u64, Bytes)>>;

/// A one-way datagram link with impairments. Datagrams arrive on the caller's thread and are
/// delivered from the link's own thread once they are due.
pub struct SimLink {
    impairments: Impairments,
    start: Instant,
    rng: Mutex<StdRng>,
    queue: Arc<(Mutex<Queue>, Condvar)>,
    order: AtomicU64,
    stats: Arc<LinkStats>,
    running: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl SimLink {
    /// Start a link that hands each delivered datagram to `deliver`.
    pub fn new(
        impairments: Impairments,
        seed: u64,
        mut deliver: impl FnMut(Bytes) + Send + 'static,
    ) -> Arc<Self> {
        let queue: Arc<(Mutex<Queue>, Condvar)> = Arc::default();
        let stats = Arc::new(LinkStats::default());
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
                            deliver(datagram);
                            stats.delivered.fetch_add(1, Ordering::Relaxed);
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

    /// Hand a datagram to the link. It is dropped, delayed, or delayed and duplicated.
    pub fn send(&self, datagram: Bytes) {
        let now = Instant::now();
        let i = self.impairments;
        self.stats.sent.fetch_add(1, Ordering::Relaxed);
        let since = now.duration_since(self.start);
        if i.blackout
            .is_some_and(|(from, to)| since >= from && since < to)
        {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut rng = self.rng.lock().unwrap();
        if rng.gen_bool(i.loss.clamp(0.0, 1.0)) {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
            return;
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
    }

    pub fn stats(&self) -> &LinkStats {
        &self.stats
    }

    fn schedule(&self, datagram: Bytes, due: Instant) {
        let (lock, wake) = &*self.queue;
        let order = self.order.fetch_add(1, Ordering::Relaxed);
        lock.lock().unwrap().push(Reverse((due, order, datagram)));
        wake.notify_one();
    }

    /// Stop delivering and join the link thread. Called from [`crate::Scenario`]'s `Drop`.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.queue.1.notify_all();
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_profile_resolves() {
        for (name, _) in PROFILES {
            assert!(Impairments::named(name).is_some(), "{name}");
        }
        assert!(Impairments::named("nonsense").is_none());
    }

    #[test]
    fn netem_carries_half_the_jitter_in_the_mean() {
        let i = Impairments::named("wifi-24ghz").unwrap();
        // 6 ms delay + 0..15 ms uniform = 13.5 ms ± 7.5 ms, rounded down to whole milliseconds.
        assert_eq!(
            i.netem_args(),
            [
                "netem",
                "delay",
                "13ms",
                "7ms",
                "distribution",
                "uniform",
                "loss",
                "1%",
            ]
        );
    }

    #[test]
    fn a_perfect_link_asks_for_nothing() {
        assert_eq!(Impairments::default().netem_args(), ["netem"]);
        assert!(Impairments::default().clumsy_args().is_empty());
    }

    #[test]
    fn clumsy_gets_the_mean_delay_and_the_loss() {
        let args = Impairments::named("lossy").unwrap().clumsy_args();
        assert_eq!(
            args,
            [
                "--lag",
                "on",
                "--lag-time",
                "10",
                "--drop",
                "on",
                "--drop-chance",
                "20"
            ]
        );
    }

    #[test]
    fn a_blackout_swallows_everything_inside_its_window() {
        let delivered = Arc::new(AtomicU64::new(0));
        let link = SimLink::new(
            Impairments {
                blackout: Some((Duration::ZERO, Duration::from_secs(60))),
                ..Impairments::default()
            },
            1,
            {
                let delivered = delivered.clone();
                move |_| {
                    delivered.fetch_add(1, Ordering::Relaxed);
                }
            },
        );
        for _ in 0..10 {
            link.send(Bytes::from_static(b"datagram"));
        }
        link.stop();
        assert_eq!(link.stats().sent(), 10);
        assert_eq!(link.stats().dropped(), 10);
        assert_eq!(delivered.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_clean_link_delivers_everything_it_is_given() {
        let delivered = Arc::new(AtomicU64::new(0));
        let link = SimLink::new(Impairments::default(), 2, {
            let delivered = delivered.clone();
            move |_| {
                delivered.fetch_add(1, Ordering::Relaxed);
            }
        });
        for _ in 0..20 {
            link.send(Bytes::from_static(b"datagram"));
        }
        // The link thread delivers from its own thread; give it a moment before stopping.
        std::thread::sleep(Duration::from_millis(200));
        link.stop();
        assert_eq!(link.stats().dropped(), 0);
        assert_eq!(delivered.load(Ordering::Relaxed), 20);
    }
}
