//! SoundPush soak and latency harness (plan §29.1, "Soak" and "Audio quality").
//!
//! Two engines run in this process and stream to each other over real QUIC (or TLS over TCP) on
//! loopback. The sending engine's capture is a generated signal: a 20 ms 1 kHz burst once per
//! second, silence otherwise. The receiving engine's render detects each burst, so every second
//! yields one end-to-end pipeline latency (capture → encode → network → jitter buffer → decode →
//! render; device buffers excluded). Every interval the harness prints and optionally writes CSV:
//! latency percentiles, bursts missed, underruns, loss, jitter, buffer, drift and memory (Linux).
//! It exits with status 1 when a budget is exceeded, so CI can run a short soak.
//!
//! See `tools/soak/README.md` for usage.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sp_engine::settings::{LatencyMode, QualityMode};
use sp_engine::sp_audio_io::{
    AudioBackend, AudioError, AudioStream, CaptureCallback, CaptureSource, DeviceInfo, DeviceKind,
    ErrorCallback, RenderCallback, RenderTarget, StreamInfo,
};
use sp_engine::state::{RouteKind, RouteStatus};
use sp_engine::{EngineConfig, EngineHandle, EngineState, PlatformHooks};

const RATE: u64 = 48_000;
const TICK: Duration = Duration::from_millis(10);
const BURST_SAMPLES: u64 = 960;
const BURST_PERIOD: u64 = RATE;
/// A burst not heard within this time counts as missed (a dropout during the burst).
const MISSED_AFTER: Duration = Duration::from_secs(2);

type Error = Box<dyn std::error::Error>;

struct Options {
    minutes: f64,
    interval: Duration,
    latency: LatencyMode,
    quality: QualityMode,
    tcp: bool,
    csv: Option<PathBuf>,
    max_latency_ms: Option<f64>,
    max_missed_per_hour: Option<f64>,
}

fn parse_args() -> Result<Options, Error> {
    let mut o = Options {
        minutes: 10.0,
        interval: Duration::from_secs(10),
        latency: LatencyMode::Balanced,
        quality: QualityMode::Auto,
        tcp: false,
        csv: None,
        max_latency_ms: None,
        max_missed_per_hour: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--minutes" => o.minutes = value()?.parse()?,
            "--interval" => o.interval = Duration::from_secs_f64(value()?.parse()?),
            "--latency" => {
                o.latency = match value()?.as_str() {
                    "low" | "lowLatency" => LatencyMode::LowLatency,
                    "balanced" => LatencyMode::Balanced,
                    "stable" => LatencyMode::Stable,
                    other => return Err(format!("unknown latency {other}").into()),
                }
            }
            "--quality" => {
                o.quality = match value()?.as_str() {
                    "auto" => QualityMode::Auto,
                    "opus" => QualityMode::Opus,
                    "lossless" => QualityMode::Lossless,
                    other => return Err(format!("unknown quality {other}").into()),
                }
            }
            "--transport" => o.tcp = value()? == "tcp",
            "--csv" => o.csv = Some(PathBuf::from(value()?)),
            "--max-latency-ms" => o.max_latency_ms = Some(value()?.parse()?),
            "--max-missed-per-hour" => o.max_missed_per_hour = Some(value()?.parse()?),
            "--help" | "-h" => {
                println!("{}", include_str!("../README.md"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other} (see --help)").into()),
        }
    }
    Ok(o)
}

// ------------------------------------------------------------------ probe audio backend

#[derive(Default)]
struct Probe {
    /// When each burst entered the sender's capture, oldest first.
    emissions: Mutex<VecDeque<Instant>>,
    latencies_ms: Mutex<Vec<f64>>,
    sent: AtomicU64,
    heard: AtomicU64,
    missed: AtomicU64,
}

impl Probe {
    fn expire(&self, now: Instant) {
        if let Ok(mut e) = self.emissions.lock() {
            while e
                .front()
                .is_some_and(|t| now.duration_since(*t) > MISSED_AFTER)
            {
                e.pop_front();
                self.missed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

struct Ticker {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    info: StreamInfo,
}

impl AudioStream for Ticker {
    fn info(&self) -> StreamInfo {
        self.info
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Call `tick` every 10 ms of wall time, like an audio device.
fn ticker(channels: u16, mut tick: impl FnMut() + Send + 'static) -> Box<dyn AudioStream> {
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    let thread = std::thread::spawn(move || {
        let start = Instant::now();
        let mut n = 0u32;
        while flag.load(Ordering::Relaxed) {
            tick();
            n += 1;
            if let Some(wait) = (start + TICK * n).checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
    });
    Box::new(Ticker {
        running,
        thread: Some(thread),
        info: StreamInfo {
            device_sample_rate: RATE as u32,
            device_channels: channels,
            channels,
            latency_ms: 10,
        },
    })
}

struct ProbeBackend(Arc<Probe>);

impl AudioBackend for ProbeBackend {
    fn name(&self) -> &'static str {
        "soak-probe"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(vec![DeviceInfo {
            id: "probe".into(),
            name: "Probe".into(),
            kind: DeviceKind::Output,
            is_default: true,
            channels: 2,
            sample_rate: RATE as u32,
        }])
    }

    fn supports_loopback(&self) -> bool {
        true
    }

    fn open_capture(
        &self,
        _source: &CaptureSource,
        channels: u16,
        mut on_audio: CaptureCallback,
        _on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let probe = self.0.clone();
        let ch = channels.clamp(1, 2) as usize;
        let mut buf = vec![0.0f32; (RATE / 100) as usize * ch];
        let mut position = 0u64;
        Ok(ticker(channels, move || {
            let now = Instant::now();
            for (i, frame) in buf.chunks_exact_mut(ch).enumerate() {
                let pos = (position + i as u64) % BURST_PERIOD;
                if pos == 0 {
                    let at = now + Duration::from_micros(i as u64 * 1_000_000 / RATE);
                    if let Ok(mut e) = probe.emissions.lock() {
                        e.push_back(at);
                        e.truncate(64);
                    }
                    probe.sent.fetch_add(1, Ordering::Relaxed);
                }
                let v = if pos < BURST_SAMPLES {
                    (2.0 * std::f32::consts::PI * 1000.0 * pos as f32 / RATE as f32).sin() * 0.5
                } else {
                    0.0
                };
                frame.fill(v);
            }
            position += RATE / 100;
            on_audio(&buf);
        }))
    }

    fn open_render(
        &self,
        _target: &RenderTarget,
        channels: u16,
        mut on_audio: RenderCallback,
        _on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let probe = self.0.clone();
        let ch = channels.clamp(1, 2) as usize;
        let mut buf = vec![0.0f32; (RATE / 100) as usize * ch];
        let mut in_burst = false;
        Ok(ticker(channels, move || {
            let now = Instant::now();
            on_audio(&mut buf);
            match buf.iter().position(|s| s.abs() > 0.1) {
                Some(index) if !in_burst => {
                    in_burst = true;
                    let onset = now + Duration::from_micros((index / ch) as u64 * 1_000_000 / RATE);
                    let emitted = probe.emissions.lock().ok().and_then(|mut e| {
                        // The burst heard is the oldest one emitted before it.
                        e.front().copied().filter(|t| *t <= onset).inspect(|_| {
                            e.pop_front();
                        })
                    });
                    if let Some(emitted) = emitted {
                        probe.heard.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut l) = probe.latencies_ms.lock() {
                            l.push(onset.duration_since(emitted).as_secs_f64() * 1000.0);
                        }
                    }
                }
                None => in_burst = false,
                _ => {}
            }
        }))
    }
}

struct Hooks {
    dir: PathBuf,
    name: &'static str,
    platform: &'static str,
    backend: Arc<ProbeBackend>,
}

impl PlatformHooks for Hooks {
    fn data_dir(&self) -> PathBuf {
        self.dir.clone()
    }
    fn storage_key(&self) -> [u8; 32] {
        [7; 32]
    }
    fn platform(&self) -> &'static str {
        self.platform
    }
    fn default_device_name(&self) -> String {
        self.name.to_string()
    }
    fn audio_backend(&self) -> Arc<dyn AudioBackend> {
        self.backend.clone()
    }
}

// ------------------------------------------------------------------ run

fn wait_for(
    engine: &EngineHandle,
    what: &str,
    pred: impl Fn(&EngineState) -> bool,
) -> Result<Arc<EngineState>, Error> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let state = engine.state();
        if pred(&state) {
            return Ok(state);
        }
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for {what}").into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn connected(s: &EngineState, peer: &str) -> bool {
    s.peers
        .iter()
        .any(|p| p.device_id == peer && p.trusted && p.connection.is_connected())
}

fn rss_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
        return line.split_whitespace().nth(1)?.parse().ok();
    }
    #[allow(unreachable_code)]
    None
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("soak failed: {e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, Error> {
    let o = parse_args()?;
    let base = std::env::temp_dir().join(format!("soundpush-soak-{}", std::process::id()));
    let probe = Arc::new(Probe::default());
    let backend = Arc::new(ProbeBackend(probe.clone()));
    let start_engine =
        |name: &'static str, platform: &'static str, include_loopback: bool, tcp_listener: bool| {
            EngineHandle::start(
                Arc::new(Hooks {
                    dir: base.join(name),
                    name,
                    platform,
                    backend: backend.clone(),
                }),
                EngineConfig {
                    app_version: "soak".into(),
                    port: 0,
                    discovery: false,
                    include_loopback,
                    tcp_listener,
                },
            )
        };
    let sender = start_engine("sender", "linux", true, true)?;
    // Over TCP the receiver acts as a phone on USB: no usable network candidates.
    let receiver = if o.tcp {
        start_engine("receiver", "android", false, false)?
    } else {
        start_engine("receiver", "linux", true, true)?
    };
    let rt = tokio::runtime::Runtime::new()?;
    let sender_id = sender.state().local.device_id.clone();
    let receiver_id = receiver.state().local.device_id.clone();

    // The returned code, not the state: snapshots are throttled and may not show it yet.
    let uri = rt.block_on(sender.start_pairing())?;
    if o.tcp {
        let port = sender.state().local.tcp_port;
        rt.block_on(receiver.pair_with_address(format!("127.0.0.1:{port}")))?;
        wait_for(&sender, "pairing code", |s| !s.pairing.prompts.is_empty())?;
        wait_for(&receiver, "pairing code", |s| !s.pairing.prompts.is_empty())?;
        sender.confirm_pairing(receiver_id.clone(), true)?;
        receiver.confirm_pairing(sender_id.clone(), true)?;
    } else {
        rt.block_on(receiver.pair_with_qr(uri))?;
    }
    wait_for(&receiver, "connection", |s| connected(s, &sender_id))?;

    let mut settings = receiver.state().settings.clone();
    settings.stream.latency = o.latency;
    settings.stream.quality = o.quality;
    rt.block_on(receiver.update_settings(settings))?;
    rt.block_on(receiver.start_route(sender_id.clone(), RouteKind::ReceiveSystemAudio, false))?;
    wait_for(&receiver, "active route", |s| {
        s.routes.iter().any(|r| r.status == RouteStatus::Active)
    })?;

    let transport = receiver
        .state()
        .peers
        .iter()
        .find(|p| p.device_id == sender_id)
        .map(|p| p.transport.clone())
        .unwrap_or_default();
    println!(
        "soak: {:.1} min, latency {:?}, quality {:?}, transport {transport}",
        o.minutes, o.latency, o.quality
    );
    let mut csv = match &o.csv {
        Some(path) => {
            let mut f = std::fs::File::create(path)?;
            writeln!(
                f,
                "elapsed_s,latency_p50_ms,latency_p95_ms,latency_max_ms,bursts_sent,bursts_missed,underruns,loss_pct,jitter_ms,buffer_ms,drift_ppm,engine_latency_ms,rss_kb"
            )?;
            Some(f)
        }
        None => None,
    };
    println!(
        "elapsed  p50ms  p95ms  maxms  sent  missed  underruns  loss%  jitter  buffer  drift  rssKB"
    );

    let started = Instant::now();
    let total = Duration::from_secs_f64(o.minutes * 60.0);
    let mut all_latencies: Vec<f64> = Vec::new();
    let mut ok = true;
    while started.elapsed() < total {
        std::thread::sleep(
            o.interval
                .min(total.saturating_sub(started.elapsed()))
                .max(Duration::from_millis(100)),
        );
        probe.expire(Instant::now());
        let mut window: Vec<f64> = probe
            .latencies_ms
            .lock()
            .map(|mut l| std::mem::take(&mut *l))
            .unwrap_or_default();
        window.sort_by(f64::total_cmp);
        all_latencies.extend_from_slice(&window);
        let state = receiver.state();
        let Some(route) = state.routes.first() else {
            eprintln!("the route stopped");
            return Ok(false);
        };
        let s = &route.stats;
        let (sent, missed) = (
            probe.sent.load(Ordering::Relaxed),
            probe.missed.load(Ordering::Relaxed),
        );
        let rss = rss_kb();
        println!(
            "{:>7.0}  {:>5.1}  {:>5.1}  {:>5.1}  {:>4}  {:>6}  {:>9}  {:>5.2}  {:>6.1}  {:>6.1}  {:>5}  {:>5}",
            started.elapsed().as_secs_f64(),
            percentile(&window, 0.5),
            percentile(&window, 0.95),
            window.last().copied().unwrap_or(0.0),
            sent,
            missed,
            s.underruns,
            s.loss_pct,
            s.jitter_ms,
            s.buffer_ms,
            s.drift_ppm,
            rss.map_or("n/a".to_string(), |r| r.to_string()),
        );
        if let Some(f) = csv.as_mut() {
            writeln!(
                f,
                "{:.0},{:.2},{:.2},{:.2},{sent},{missed},{},{:.3},{:.2},{:.2},{},{:.2},{}",
                started.elapsed().as_secs_f64(),
                percentile(&window, 0.5),
                percentile(&window, 0.95),
                window.last().copied().unwrap_or(0.0),
                s.underruns,
                s.loss_pct,
                s.jitter_ms,
                s.buffer_ms,
                s.drift_ppm,
                s.latency_ms,
                rss.map_or(String::new(), |r| r.to_string()),
            )?;
        }
    }

    all_latencies.sort_by(f64::total_cmp);
    let hours = started.elapsed().as_secs_f64() / 3600.0;
    let missed = probe.missed.load(Ordering::Relaxed);
    let missed_per_hour = missed as f64 / hours.max(1e-9);
    let p95 = percentile(&all_latencies, 0.95);
    println!(
        "summary: bursts {} heard {} missed {missed} ({missed_per_hour:.1}/h), latency p50 {:.1} ms p95 {p95:.1} ms max {:.1} ms",
        probe.sent.load(Ordering::Relaxed),
        probe.heard.load(Ordering::Relaxed),
        percentile(&all_latencies, 0.5),
        all_latencies.last().copied().unwrap_or(0.0),
    );
    if all_latencies.is_empty() {
        eprintln!("FAIL: no burst was heard");
        ok = false;
    }
    if let Some(max) = o.max_latency_ms.filter(|max| p95 > *max) {
        eprintln!("FAIL: p95 latency {p95:.1} ms > {max} ms");
        ok = false;
    }
    if let Some(max) = o.max_missed_per_hour.filter(|max| missed_per_hour > *max) {
        eprintln!("FAIL: {missed_per_hour:.1} missed bursts per hour > {max}");
        ok = false;
    }

    sender.shutdown(Duration::from_secs(2));
    receiver.shutdown(Duration::from_secs(2));
    let _ = std::fs::remove_dir_all(&base);
    Ok(ok)
}
