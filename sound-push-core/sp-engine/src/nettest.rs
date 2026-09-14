//! Network self-test between two connected devices (plan §28.2).
//!
//! Runs over the datagram path the audio uses, so it measures what streaming will see:
//! 1. **Latency phase:** small probes every 20 ms; the peer echoes each one. Gives RTT (median,
//!    95th percentile), jitter (mean change between consecutive RTTs) and loss.
//! 2. **Throughput phase:** bursts of full-size probes at increasing rates (128 kb/s up to
//!    1.6 Mb/s, enough for lossless stereo). The highest rate delivered with ≥ 97 % of probes
//!    echoed is the achievable bitrate. The phase stops at the first rate that fails.
//!
//! The test is bounded (about ten seconds), cancellable (the engine aborts the task) and the
//! responder only echoes probes of a test it accepted, rate-limited, without padding.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use sp_media::profile::OPUS_BITRATE_STEPS;
use sp_protocol::probe::{MAX_PROBE_LEN, Probe, ProbeKind};
use tokio::sync::mpsc;

use crate::error::ErrorView;
use crate::pipeline::sender::DatagramSink;
use crate::session::ProbeEcho;
use crate::settings::{LatencyMode, QualityMode};

/// How long a responder keeps echoing after accepting a test.
pub const RESPONDER_WINDOW: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct TestPlan {
    pub latency_probes: u32,
    pub probe_interval: Duration,
    pub steps_kbps: Vec<u32>,
    pub step_duration: Duration,
    pub probe_bytes: usize,
    /// Time to wait for late echoes after each phase.
    pub linger: Duration,
}

impl Default for TestPlan {
    fn default() -> Self {
        Self {
            latency_probes: 150,
            probe_interval: Duration::from_millis(20),
            steps_kbps: vec![128, 320, 640, 1100, 1600],
            step_duration: Duration::from_millis(800),
            probe_bytes: 1000,
            linger: Duration::from_millis(400),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkReport {
    /// "quic" or "tcp".
    pub transport: String,
    pub rtt_ms: f64,
    pub rtt_p95_ms: f64,
    pub jitter_ms: f64,
    pub loss_pct: f64,
    /// Highest bitrate delivered reliably (kb/s); 0 when even the lowest step failed.
    pub achievable_kbps: u32,
    /// Largest datagram the path accepts right now (MTU check).
    pub max_datagram_bytes: u32,
    pub probes_sent: u32,
    pub probes_received: u32,
    pub duration_ms: u32,
    pub recommendation: Recommendation,
}

/// Settings the test suggests for this device. The UI can apply them as the device's profile.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub latency: LatencyMode,
    pub quality: QualityMode,
    pub opus_bitrate: u32,
    pub redundancy: bool,
    /// Localization keys (`nettest.tip.*`), most important first.
    pub tips: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkTestStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkTestView {
    pub peer_id: String,
    pub status: NetworkTestStatus,
    /// 0.0 – 1.0.
    pub progress: f32,
    pub report: Option<NetworkReport>,
    pub error: Option<ErrorView>,
    pub started_unix: u64,
}

/// Latency-phase numbers from sent probe count and measured round-trip times (in seq order).
pub(crate) fn summarize(sent: u32, rtts_ms: &[f64]) -> (f64, f64, f64, f64) {
    let received = rtts_ms.len() as f64;
    let loss_pct = if sent > 0 {
        ((sent as f64 - received) / sent as f64 * 100.0).max(0.0)
    } else {
        0.0
    };
    if rtts_ms.is_empty() {
        return (0.0, 0.0, 0.0, loss_pct);
    }
    let mut sorted = rtts_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    let pick = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    let jitter = if rtts_ms.len() > 1 {
        rtts_ms.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (rtts_ms.len() - 1) as f64
    } else {
        0.0
    };
    (pick(0.5), pick(0.95), jitter, loss_pct)
}

/// Turn measurements into suggested settings.
pub fn recommend(
    rtt_p95_ms: f64,
    jitter_ms: f64,
    loss_pct: f64,
    achievable_kbps: u32,
) -> Recommendation {
    let mut tips = Vec::new();
    let latency = if loss_pct > 3.0 || jitter_ms > 20.0 || rtt_p95_ms > 60.0 {
        LatencyMode::Stable
    } else if loss_pct > 0.5 || jitter_ms > 6.0 || rtt_p95_ms > 15.0 {
        LatencyMode::Balanced
    } else {
        LatencyMode::LowLatency
    };
    if loss_pct > 3.0 {
        tips.push("nettest.tip.loss".to_string());
    }
    if jitter_ms > 20.0 {
        tips.push("nettest.tip.jitter".to_string());
    }
    if rtt_p95_ms > 60.0 {
        tips.push("nettest.tip.latency".to_string());
    }

    // Automatic quality adapts to the link when there is room for its 128 kb/s start; lossless
    // (≈1.55 Mb/s) is only mentioned as possible, never chosen for the user.
    let (quality, opus_bitrate) = if achievable_kbps >= 320 {
        (QualityMode::Auto, 128_000)
    } else {
        // Leave half the measured capacity for retransmissions and other traffic.
        let budget = achievable_kbps.saturating_mul(1000) / 2;
        let bitrate = OPUS_BITRATE_STEPS
            .iter()
            .copied()
            .filter(|b| *b <= budget)
            .max()
            .unwrap_or(OPUS_BITRATE_STEPS[0]);
        tips.push("nettest.tip.bandwidth".to_string());
        (QualityMode::Opus, bitrate)
    };
    if achievable_kbps >= 1600 && loss_pct < 0.2 {
        tips.push("nettest.tip.losslessOk".to_string());
    }
    if tips.is_empty() || tips == ["nettest.tip.losslessOk"] {
        tips.insert(0, "nettest.tip.good".to_string());
    }
    Recommendation {
        latency,
        quality,
        opus_bitrate,
        redundancy: loss_pct > 1.0,
        tips,
    }
}

/// Progress in permille, shared with the engine for the state snapshot.
pub(crate) type Progress = Arc<AtomicU32>;

/// Run the initiator side. `echoes` receives echoes of this test's probes.
pub(crate) async fn run(
    sink: Arc<dyn DatagramSink>,
    transport: &'static str,
    max_datagram_bytes: u32,
    test_id: u32,
    mut echoes: mpsc::Receiver<ProbeEcho>,
    progress: Progress,
    plan: TestPlan,
) -> NetworkReport {
    let started = Instant::now();
    let epoch = Instant::now();
    let now_us = || epoch.elapsed().as_micros() as u64;
    let steps = plan.steps_kbps.len() as u32;
    let probe = |seq: u32, sent_us: u64| Probe {
        kind: ProbeKind::Probe,
        test_id,
        seq,
        sent_us,
        echo_us: 0,
    };
    let mut probes_sent = 0u32;
    let mut probes_received = 0u32;

    // Phase 1: latency.
    let mut rtts: HashMap<u32, f64> = HashMap::new();
    let mut tick = tokio::time::interval(plan.probe_interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    for seq in 0..plan.latency_probes {
        tick.tick().await;
        if sink.send(probe(seq, now_us()).encode(64)).is_ok() {
            probes_sent += 1;
        }
        drain(&mut echoes, epoch, test_id, |seq, rtt| {
            rtts.entry(seq).or_insert(rtt);
        });
        progress.store(seq * 400 / plan.latency_probes.max(1), Ordering::Relaxed);
    }
    linger(&mut echoes, epoch, test_id, plan.linger, |seq, rtt| {
        rtts.entry(seq).or_insert(rtt);
    })
    .await;
    rtts.retain(|seq, _| *seq < plan.latency_probes);
    probes_received += rtts.len() as u32;
    let mut ordered: Vec<(u32, f64)> = rtts.into_iter().collect();
    ordered.sort_by_key(|(seq, _)| *seq);
    let rtt_list: Vec<f64> = ordered.into_iter().map(|(_, r)| r).collect();
    let (rtt_ms, rtt_p95_ms, jitter_ms, loss_pct) = summarize(plan.latency_probes, &rtt_list);

    // Phase 2: throughput, skipped when the link is already failing.
    let mut achievable_kbps = 0;
    if loss_pct < 30.0 {
        let size = plan.probe_bytes.clamp(64, MAX_PROBE_LEN);
        for (i, kbps) in plan.steps_kbps.iter().copied().enumerate() {
            let base = 1_000_000 * (i as u32 + 1);
            let per_sec = (kbps as usize * 1000 / 8 / size).max(1);
            let count = (per_sec as f64 * plan.step_duration.as_secs_f64()).ceil() as u32;
            let interval = plan.step_duration / count.max(1);
            let mut tick = tokio::time::interval(interval);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
            let mut delivered = std::collections::HashSet::new();
            for n in 0..count {
                tick.tick().await;
                if sink.send(probe(base + n, now_us()).encode(size)).is_ok() {
                    probes_sent += 1;
                }
                drain(&mut echoes, epoch, test_id, |seq, _| {
                    if seq >= base && seq < base + count {
                        delivered.insert(seq);
                    }
                });
            }
            linger(&mut echoes, epoch, test_id, plan.linger, |seq, _| {
                if seq >= base && seq < base + count {
                    delivered.insert(seq);
                }
            })
            .await;
            probes_received += delivered.len() as u32;
            progress.store(400 + (i as u32 + 1) * 600 / steps.max(1), Ordering::Relaxed);
            if delivered.len() as f64 >= count as f64 * 0.97 {
                achievable_kbps = kbps;
            } else {
                break;
            }
        }
    }
    progress.store(1000, Ordering::Relaxed);

    NetworkReport {
        transport: transport.to_string(),
        rtt_ms,
        rtt_p95_ms,
        jitter_ms,
        loss_pct,
        achievable_kbps,
        max_datagram_bytes,
        probes_sent,
        probes_received,
        duration_ms: started.elapsed().as_millis() as u32,
        recommendation: recommend(rtt_p95_ms, jitter_ms, loss_pct, achievable_kbps),
    }
}

fn drain(
    echoes: &mut mpsc::Receiver<ProbeEcho>,
    epoch: Instant,
    test_id: u32,
    mut on: impl FnMut(u32, f64),
) {
    while let Ok(e) = echoes.try_recv() {
        if let Some(rtt) = rtt_of(&e, epoch, test_id) {
            on(e.probe.seq, rtt);
        }
    }
}

async fn linger(
    echoes: &mut mpsc::Receiver<ProbeEcho>,
    epoch: Instant,
    test_id: u32,
    wait: Duration,
    mut on: impl FnMut(u32, f64),
) {
    let deadline = tokio::time::Instant::now() + wait;
    while let Ok(Some(e)) = tokio::time::timeout_at(deadline, echoes.recv()).await {
        if let Some(rtt) = rtt_of(&e, epoch, test_id) {
            on(e.probe.seq, rtt);
        }
    }
}

fn rtt_of(e: &ProbeEcho, epoch: Instant, test_id: u32) -> Option<f64> {
    if e.probe.kind != ProbeKind::Echo || e.probe.test_id != test_id {
        return None;
    }
    let arrived = e.received.checked_duration_since(epoch)?.as_micros() as u64;
    // A forged or corrupted sent_us from the future is not a measurement.
    arrived
        .checked_sub(e.probe.sent_us)
        .map(|us| us as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use bytes::Bytes;

    use super::*;

    #[test]
    fn summary_numbers() {
        let (median, p95, jitter, loss) = summarize(10, &[2.0, 4.0, 2.0, 4.0, 2.0]);
        assert_eq!(median, 2.0);
        assert_eq!(p95, 4.0);
        assert_eq!(jitter, 2.0);
        assert_eq!(loss, 50.0);
        assert_eq!(summarize(0, &[]), (0.0, 0.0, 0.0, 0.0));
        assert_eq!(summarize(4, &[]).3, 100.0);
    }

    #[test]
    fn recommendations_follow_the_link() {
        let lan = recommend(3.0, 1.0, 0.0, 1600);
        assert_eq!(lan.latency, LatencyMode::LowLatency);
        assert!(!lan.redundancy);
        assert_eq!(lan.tips[0], "nettest.tip.good");
        assert!(lan.tips.contains(&"nettest.tip.losslessOk".to_string()));

        let busy = recommend(25.0, 8.0, 1.5, 640);
        assert_eq!(busy.latency, LatencyMode::Balanced);
        assert!(busy.redundancy);

        let weak = recommend(120.0, 35.0, 8.0, 128);
        assert_eq!(weak.latency, LatencyMode::Stable);
        assert_eq!(weak.quality, QualityMode::Opus);
        assert!(weak.opus_bitrate <= 64_000);
        assert!(weak.tips.contains(&"nettest.tip.loss".to_string()));
        assert!(weak.tips.contains(&"nettest.tip.bandwidth".to_string()));

        let dead = recommend(0.0, 0.0, 100.0, 0);
        assert_eq!(dead.opus_bitrate, OPUS_BITRATE_STEPS[0]);
    }

    /// Echoes every probe back after an optional drop, in-process.
    struct Reflector {
        echoes: mpsc::Sender<ProbeEcho>,
        drop_every: u32,
        counter: Mutex<u32>,
    }

    impl DatagramSink for Reflector {
        fn send(&self, datagram: Bytes) -> Result<(), ()> {
            let p = Probe::decode(&datagram).map_err(|_| ())?;
            let mut n = self.counter.lock().map_err(|_| ())?;
            *n += 1;
            if self.drop_every > 0 && *n % self.drop_every == 0 {
                return Ok(());
            }
            let _ = self.echoes.try_send(ProbeEcho {
                probe: p.echo(0),
                received: Instant::now(),
            });
            Ok(())
        }
    }

    fn quick_plan() -> TestPlan {
        TestPlan {
            latency_probes: 40,
            probe_interval: Duration::from_millis(2),
            steps_kbps: vec![128, 320],
            step_duration: Duration::from_millis(100),
            probe_bytes: 200,
            linger: Duration::from_millis(50),
        }
    }

    #[tokio::test]
    async fn clean_path_reports_no_loss_and_full_throughput() {
        let (tx, rx) = mpsc::channel(4096);
        let sink = Arc::new(Reflector {
            echoes: tx,
            drop_every: 0,
            counter: Mutex::new(0),
        });
        let progress = Arc::new(AtomicU32::new(0));
        let report = run(sink, "quic", 1100, 5, rx, progress.clone(), quick_plan()).await;
        assert_eq!(report.loss_pct, 0.0);
        assert_eq!(report.achievable_kbps, 320);
        assert_eq!(progress.load(Ordering::Relaxed), 1000);
        assert_eq!(report.probes_sent, report.probes_received);
    }

    #[tokio::test]
    async fn lossy_path_is_detected() {
        let (tx, rx) = mpsc::channel(4096);
        let sink = Arc::new(Reflector {
            echoes: tx,
            drop_every: 5,
            counter: Mutex::new(0),
        });
        let report = run(
            sink,
            "tcp",
            1100,
            5,
            rx,
            Arc::new(AtomicU32::new(0)),
            quick_plan(),
        )
        .await;
        assert!(
            (report.loss_pct - 20.0).abs() < 3.0,
            "loss {}",
            report.loss_pct
        );
        assert_eq!(report.achievable_kbps, 0);
        assert!(report.recommendation.redundancy);
    }
}
