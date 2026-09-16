//! End-to-end media pipeline latency from a chirp and its cross-correlation (plan §11.1
//! `tools/latency-probe`, §29.1 "Audio quality").
//!
//! A real sender and receiver pipeline runs over a simulated path (`sp-testkit`). The sender's
//! capture emits a 20 ms chirp once a second; the receiver's render is recorded. Each period of the
//! recording is correlated with the chirp, which gives one latency reading per second, and both
//! streams timestamp their first buffer so the readings sit on a common clock.
//!
//! See `tools/latency-probe/README.md` for usage.

use std::process::ExitCode;
use std::time::Duration;

use sp_media::profile::{LatencyProfile, Quality, build_profile};
use sp_testkit::audio::{RATE, chirp_period, correlation_shift, peak, to_mono, with_tone_floor};
use sp_testkit::{Impairments, Scenario, Source};

type Error = Box<dyn std::error::Error>;

/// One reading per second, with a 20 ms chirp at the start of each period.
const PERIOD: usize = RATE;
const BURST: usize = RATE / 50;
/// Readings from the first periods are discarded: the jitter buffer is still finding its target.
const WARMUP_PERIODS: usize = 2;
/// Level of the tone that keeps the stream out of DTX between bursts.
const TONE_FLOOR: f32 = 0.02;
/// A period whose peak stays near the tone floor never carried a chirp.
const HEARD: f32 = 0.1;

struct Options {
    latency: LatencyProfile,
    quality: Quality,
    channels: u32,
    redundancy: bool,
    link: String,
    seconds: f64,
    seed: u64,
    max_ms: Option<f64>,
    json: bool,
}

fn parse_args() -> Result<Options, Error> {
    let mut o = Options {
        latency: LatencyProfile::Balanced,
        quality: Quality::Auto,
        channels: 2,
        redundancy: false,
        link: "perfect".into(),
        seconds: 12.0,
        seed: 1,
        max_ms: None,
        json: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--latency" => {
                o.latency = match value()?.as_str() {
                    "low" | "lowLatency" => LatencyProfile::LowLatency,
                    "balanced" => LatencyProfile::Balanced,
                    "stable" => LatencyProfile::Stable,
                    other => return Err(format!("unknown latency {other}").into()),
                }
            }
            "--quality" => {
                let v = value()?;
                o.quality = match v.split_once(':') {
                    Some(("opus", bps)) => Quality::Opus(bps.parse()?),
                    _ => match v.as_str() {
                        "auto" => Quality::Auto,
                        "opus" => Quality::Opus(128_000),
                        "lossless" | "pcm" => Quality::Lossless,
                        other => return Err(format!("unknown quality {other}").into()),
                    },
                }
            }
            "--channels" => o.channels = value()?.parse()?,
            "--redundancy" => o.redundancy = true,
            "--link" => o.link = value()?,
            "--seconds" => o.seconds = value()?.parse()?,
            "--seed" => o.seed = value()?.parse()?,
            "--max-ms" => o.max_ms = Some(value()?.parse()?),
            "--json" => o.json = true,
            "--help" | "-h" => {
                println!("{}", include_str!("../README.md"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other} (see --help)").into()),
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("latency-probe: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `Ok(false)` when the run finished but missed its budget.
fn run() -> Result<bool, Error> {
    let o = parse_args()?;
    let impairments = Impairments::named(&o.link)
        .ok_or_else(|| format!("unknown link profile {} (see `netsim list`)", o.link))?;
    let reference = chirp_period(PERIOD, BURST, 500.0, 6_000.0);
    // The signal on the wire is the chirp over a quiet tone; only the chirp is correlated.
    let signal = with_tone_floor(reference.clone(), 220.0, TONE_FLOOR);
    let profile = build_profile(o.latency, o.quality, o.channels, o.redundancy);
    let scenario = Scenario::start(
        profile,
        impairments,
        o.redundancy,
        o.seed,
        Source::Signal(signal),
    );
    scenario.run_for(Duration::from_secs_f64(o.seconds.max(3.0)));

    let channels = scenario.channels();
    let recorded = to_mono(&scenario.recorded(), channels);
    let marks = scenario
        .marks()
        .ok_or("the scenario did not record its stream start times")?;
    let (capture_start, render_start) = match (marks.capture_started(), marks.render_started()) {
        (Some(c), Some(r)) => (c, r),
        _ => return Err("capture or render never produced a buffer".into()),
    };
    // Positive when the recording starts after the capture does; either sign is fine.
    let offset = if render_start >= capture_start {
        (render_start - capture_start).as_secs_f64()
    } else {
        -(capture_start - render_start).as_secs_f64()
    };

    let period_secs = PERIOD as f64 / RATE as f64;
    let mut readings = Vec::new();
    let mut silent = 0usize;
    let mut period = WARMUP_PERIODS;
    while (period + 1) * PERIOD <= recorded.len() {
        let base = period * PERIOD;
        let window = &recorded[base..base + PERIOD];
        period += 1;
        if peak(window) < HEARD {
            silent += 1;
            continue;
        }
        let Some(shift) = correlation_shift(window, &reference) else {
            silent += 1;
            continue;
        };
        let latency = (offset + (base + shift) as f64 / RATE as f64).rem_euclid(period_secs);
        readings.push(latency * 1000.0);
    }
    if readings.is_empty() {
        return Err("no chirp was heard: nothing arrived through the pipeline".into());
    }
    readings.sort_by(f64::total_cmp);

    let p = |q: f64| readings[((readings.len() - 1) as f64 * q).round() as usize];
    let p50 = p(0.5);
    let p95 = p(0.95);
    let min = readings[0];
    let max = readings[readings.len() - 1];
    let stats = scenario.link().stats();
    let underruns = scenario.underruns();
    let ok = o.max_ms.is_none_or(|budget| p95 <= budget);

    if o.json {
        println!(
            concat!(
                r#"{{"link":"{}","readings":{},"missed":{},"min_ms":{:.1},"p50_ms":{:.1},"#,
                r#""p95_ms":{:.1},"max_ms":{:.1},"underruns":{},"sent":{},"dropped":{},"ok":{}}}"#
            ),
            o.link,
            readings.len(),
            silent,
            min,
            p50,
            p95,
            max,
            underruns,
            stats.sent(),
            stats.dropped(),
            ok
        );
    } else {
        println!("link {} over {:.0} s", o.link, o.seconds);
        println!(
            "  latency   min {min:.1} ms   p50 {p50:.1} ms   p95 {p95:.1} ms   max {max:.1} ms   ({} readings)",
            readings.len()
        );
        println!(
            "  path      sent {}   dropped {}   duplicated {}   underruns {underruns}   chirps missed {silent}",
            stats.sent(),
            stats.dropped(),
            stats.duplicated()
        );
    }
    if let Some(budget) = o.max_ms {
        if !ok {
            eprintln!("latency-probe: p95 {p95:.1} ms is over the {budget:.1} ms budget");
        }
    }
    Ok(ok)
}
