//! Hot-path benchmarks (plan §22.2: "Criterion benches for codec, jitter buffer, resampler").
//!
//! Run with `cargo bench -p sp-media`. Budgets from plan §22.1, per 10 ms frame (100 frames/s), so
//! 1 % of one core is 100 µs per frame:
//!
//! | Bench | Budget | Why |
//! |---|---|---|
//! | `opus/encode_stereo_128k_10ms` | < 300 µs | "One Opus 128 kb/s stereo send < 3 % of one core" |
//! | `opus/decode_stereo_10ms` | < 500 µs | "Android receive (Opus) < 5 % CPU" (decode is most of it) |
//! | `dsp/rnnoise_mono_10ms` | < 200 µs | "RNNoise on desktop < 2 % of one core per mono stream" |
//! | `dsp/mic_chain_mono_10ms` (high-pass, gain, limiter, meter) | < 20 µs | a small part of the send budget |
//! | `jitter/push_pop_10ms` | < 20 µs | receive path, runs in the render callback |
//! | `drift/resample_stereo_10ms` | < 50 µs | receive path, runs in the render callback |
//!
//! Regressions over 10 % against the previous run are what CI is meant to flag (plan §22.2).
#![allow(clippy::unwrap_used, missing_docs)]

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use sp_media::codec::{OpusApplication, decoder_for, encoder_for};
use sp_media::drift::{DriftController, FractionalResampler};
use sp_media::dsp::{Gain, HighPass, LevelMeter, NoiseSuppressor, SoftLimiter, db_to_gain};
use sp_media::jitter::{JitterBuffer, JitterConfig};
use sp_media::profile::{LatencyProfile, Quality, build_profile};
use sp_media::samples_per_frame;

fn tone(frames: usize, channels: usize, offset: usize) -> Vec<f32> {
    (0..frames * channels)
        .map(|i| {
            let n = (i / channels + offset) as f32;
            // Two tones and a little noise-like content, closer to music than a pure sine.
            (n * 0.0576).sin() * 0.3
                + (n * 0.4213).sin() * 0.1
                + ((n * 12.9898).sin() * 43758.5).fract() * 0.02
        })
        .collect()
}

fn opus(c: &mut Criterion) {
    let profile = build_profile(LatencyProfile::Balanced, Quality::Opus(128_000), 2, false);
    let frame = samples_per_frame(profile.frame_us);
    let input: Vec<Vec<f32>> = (0..50).map(|i| tone(frame, 2, i * frame)).collect();

    let mut group = c.benchmark_group("opus");
    let mut enc = encoder_for(&profile, OpusApplication::LowDelay).unwrap();
    let mut packet = Vec::with_capacity(1500);
    let mut i = 0;
    group.bench_function("encode_stereo_128k_10ms", |b| {
        b.iter(|| {
            enc.encode(black_box(&input[i % input.len()]), &mut packet)
                .unwrap();
            i += 1;
        })
    });

    let packets: Vec<Vec<u8>> = input
        .iter()
        .map(|pcm| {
            let mut p = Vec::new();
            enc.encode(pcm, &mut p).unwrap();
            p
        })
        .collect();
    let mut dec = decoder_for(&profile).unwrap();
    let mut out = vec![0.0f32; 5760 * 2];
    let mut j = 0;
    group.bench_function("decode_stereo_10ms", |b| {
        b.iter(|| {
            dec.decode(black_box(&packets[j % packets.len()]), &mut out)
                .unwrap();
            j += 1;
        })
    });
    group.bench_function("conceal_stereo_10ms", |b| {
        b.iter(|| dec.conceal(black_box(&mut out[..frame * 2])))
    });
    group.finish();
}

fn dsp(c: &mut Criterion) {
    let mut group = c.benchmark_group("dsp");
    let input = tone(480, 1, 0);
    let mut buf = input.clone();

    let mut hp = HighPass::new(80.0, 1);
    let mut gain = Gain::new(db_to_gain(6.0));
    let limiter = SoftLimiter::default();
    let mut meter = LevelMeter::default();
    group.bench_function("mic_chain_mono_10ms", |b| {
        b.iter(|| {
            buf.copy_from_slice(&input);
            hp.process(&mut buf);
            gain.process(&mut buf);
            black_box(limiter.process(&mut buf));
            meter.process(&buf);
        })
    });

    let mut ns = NoiseSuppressor::new();
    group.bench_function("rnnoise_mono_10ms", |b| {
        b.iter(|| {
            buf.copy_from_slice(&input);
            ns.process(black_box(&mut buf));
        })
    });
    group.finish();
}

fn jitter(c: &mut Criterion) {
    let mut group = c.benchmark_group("jitter");
    for channels in [1usize, 2] {
        let frame = 480usize;
        let mut jb = JitterBuffer::new(JitterConfig {
            channels,
            frame_samples: frame,
            min_delay_ms: 20,
            max_delay_ms: 80,
        });
        let samples = vec![0.1f32; frame * channels];
        let mut out = vec![0.0f32; frame * channels];
        let mut ts = 0u64;
        // Prime past the target so every iteration plays a frame.
        for _ in 0..8 {
            jb.push(ts, samples.clone(), ts);
            ts += frame as u64;
        }
        group.bench_with_input(
            BenchmarkId::new("push_pop_10ms", channels),
            &channels,
            |b, _| {
                b.iter(|| {
                    // Arrival jitters by up to 2 ms around the send time.
                    let arrival = ts + (ts / frame as u64 % 5) * 20;
                    jb.push(ts, samples.clone(), arrival);
                    ts += frame as u64;
                    black_box(jb.pop(&mut out));
                })
            },
        );
    }
    group.finish();
}

fn drift(c: &mut Criterion) {
    let mut group = c.benchmark_group("drift");
    let input = tone(480, 2, 0);
    let mut resampler = FractionalResampler::new(2);
    let mut controller = DriftController::new();
    let mut acc = Vec::with_capacity(4 * 5760 * 2);
    group.bench_function("resample_stereo_10ms", |b| {
        b.iter(|| {
            let ratio = controller.update(black_box(2_000), 1_920);
            resampler.process(&input, ratio, &mut acc);
            acc.clear();
        })
    });
    group.finish();
}

criterion_group!(benches, opus, dsp, jitter, drift);
criterion_main!(benches);
