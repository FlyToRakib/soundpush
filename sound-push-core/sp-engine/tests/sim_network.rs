//! Simulated network between a real sender and receiver pipeline (plan §29.1 "Simulation"):
//! loss, jitter (and the reordering it causes), duplication and blackouts are injected at the
//! transport boundary, where media datagrams leave the encoder group and reach the receiver.
//! Everything else is the production pipeline: capture, DSP, Opus, redundancy, jitter buffer,
//! concealment, drift compensation and render.
//!
//! The link and the scenario live in `sp-testkit`, so `tools/latency-probe` measures the same
//! pipeline over the same impairments. The impairments are drawn from a seeded RNG, so a failure
//! reproduces. The audio devices tick in real time, so each scenario runs for a few seconds; the
//! tests run in parallel.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers fail the test on purpose

use std::sync::atomic::Ordering;
use std::time::Duration;

use sp_media::profile::{LatencyProfile, Quality, build_profile};
use sp_testkit::{Impairments, Scenario};

#[test]
fn redundancy_rides_out_random_loss() {
    let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 2, true);
    let s = Scenario::tone(
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
    let dropped = s.link().stats().dropped();
    let recovered = s.receiver().packets_recovered.load(Ordering::Relaxed);
    assert!(dropped > 10, "the link dropped packets ({dropped})");
    assert!(
        recovered * 2 >= dropped,
        "most single losses are rebuilt from the redundant copy: {recovered} of {dropped}"
    );
    assert!(s.recent_peak(500) > 0.1, "audio keeps playing");
    assert!(s.underruns() <= 3, "underruns {}", s.underruns());
    assert_eq!(s.receiver().decode_errors.load(Ordering::Relaxed), 0);
}

#[test]
fn jitter_and_reordering_are_absorbed_by_the_buffer() {
    // Up to 40 ms of jitter at 10 ms frames reorders packets constantly.
    let profile = build_profile(LatencyProfile::Stable, Quality::Auto, 2, false);
    let s = Scenario::tone(
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
    let target = s.receiver().target_ms.get();
    assert!(
        (20.0..=250.0).contains(&target),
        "target within the Stable bounds: {target}"
    );
}

#[test]
fn duplicated_packets_are_harmless() {
    let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 2, false);
    let s = Scenario::tone(
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
    assert!(s.link().stats().duplicated() > 20);
    assert!(s.recent_peak(500) > 0.1);
    assert!(s.underruns() <= 2, "underruns {}", s.underruns());
    assert_eq!(s.receiver().decode_errors.load(Ordering::Relaxed), 0);
}

#[test]
fn audio_returns_after_a_blackout() {
    let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 2, false);
    let s = Scenario::tone(
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
    let during = s.receiver().packets_received.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(1700));
    assert!(s.receiver().packets_received.load(Ordering::Relaxed) > during + 50);
    assert!(
        s.recent_peak(500) > 0.1,
        "audio resumes within a second of the path coming back"
    );
    // A 800 ms outage is one rebuffer, not a storm.
    assert!(s.underruns() <= 3, "underruns {}", s.underruns());
    assert!(
        s.receiver().target_ms.get() <= 80.0,
        "target stays within Balanced"
    );
}
