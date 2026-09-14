//! Against a real PulseAudio (or pipewire-pulse) server: speakers muting, per-app capture and
//! device events. Needs `pacat` and `pactl`; run one test at a time, they share the server:
//!
//! ```sh
//! pulseaudio --daemonize --exit-idle-time=-1
//! cargo test -p sp-audio-io --features pulse --test pulse_server -- --ignored --test-threads=1
//! ```
#![cfg(all(target_os = "linux", feature = "pulse"))]
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers fail the test on purpose

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sp_audio_io::pulse::{self, Pulse};
use sp_audio_io::{AudioStream, CaptureSource};

const SPEAKERS: &str = "sp_test_speakers";

fn pactl(args: &[&str]) -> String {
    let out = Command::new("pactl").args(args).output().expect("pactl");
    assert!(out.status.success(), "pactl {args:?} failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A null sink standing in for real speakers, made the default.
fn fake_speakers() {
    let _ = pulse::unmute_speakers();
    if !pactl(&["list", "short", "sinks"]).contains(SPEAKERS) {
        pactl(&[
            "load-module",
            "module-null-sink",
            &format!("sink_name={SPEAKERS}"),
            "rate=48000",
        ]);
    }
    pactl(&["set-default-sink", SPEAKERS]);
}

/// `pacat` playing a 440 Hz tone of `amplitude` as the app `name`.
struct Tone(Child);

impl Tone {
    fn play(name: &str, amplitude: f32) -> Self {
        let mut child = Command::new("pacat")
            .args([
                "--playback",
                "--raw",
                "--format=float32le",
                "--rate=48000",
                "--channels=2",
                "--latency-msec=20",
                &format!("--property=application.process.binary={name}"),
                &format!("--property=application.name={name}"),
            ])
            .stdin(Stdio::piped())
            .spawn()
            .expect("pacat");
        let mut stdin = child.stdin.take().expect("stdin");
        std::thread::spawn(move || {
            let mut phase = 0f32;
            let mut chunk = Vec::with_capacity(4800 * 8);
            loop {
                chunk.clear();
                for _ in 0..4800 {
                    let s = (phase * std::f32::consts::TAU).sin() * amplitude;
                    phase = (phase + 440.0 / 48_000.0) % 1.0;
                    chunk.extend_from_slice(&s.to_le_bytes());
                    chunk.extend_from_slice(&s.to_le_bytes());
                }
                // pacat blocks while its buffer is full, which paces this in real time.
                if stdin.write_all(&chunk).is_err() {
                    return;
                }
            }
        });
        let tone = Self(child);
        wait_for(|| app_stream(name).is_some());
        tone
    }
}

impl Drop for Tone {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Name of the sink the app's stream plays on.
fn app_stream(app: &str) -> Option<String> {
    let mut pulse = Pulse::connect().ok()?;
    let input = pulse
        .sink_inputs()
        .ok()?
        .into_iter()
        .find(|i| i.app == app)?;
    pulse
        .sinks()
        .ok()?
        .into_iter()
        .find(|s| s.index == input.sink)
        .map(|s| s.name)
}

/// Peak level recorded from `source` over half a second (after the stream settles).
fn peak(source: &CaptureSource) -> f32 {
    let level = Arc::new(Mutex::new(0f32));
    let meter = level.clone();
    let stream: Box<dyn AudioStream> = pulse::open_capture(
        source,
        2,
        Box::new(move |frames| {
            if let Ok(mut peak) = meter.lock() {
                *peak = frames.iter().fold(*peak, |p, s| p.max(s.abs()));
            }
        }),
        Box::new(|e| panic!("capture failed: {e}")),
    )
    .expect("open capture");
    wake_speakers();
    std::thread::sleep(Duration::from_millis(300));
    *level.lock().unwrap() = 0.0;
    std::thread::sleep(Duration::from_millis(500));
    drop(stream);
    *level.lock().unwrap()
}

/// The test speakers are a null sink, which renders in two-second blocks after it starts playing
/// until it restarts (real speakers do not); restart it so its monitor is heard right away.
fn wake_speakers() {
    pactl(&["suspend-sink", SPEAKERS, "1"]);
    pactl(&["suspend-sink", SPEAKERS, "0"]);
}

fn speakers_monitor() -> CaptureSource {
    CaptureSource::Input(format!("{SPEAKERS}.monitor"))
}

fn soundpush_modules() -> usize {
    pactl(&["list", "short", "modules"])
        .lines()
        .filter(|l| l.contains("soundpush_"))
        .count()
}

#[test]
#[ignore = "needs a PulseAudio server"]
fn muted_speakers_are_silent_while_system_audio_is_captured() {
    fake_speakers();
    let _tone = Tone::play("sp-test-music", 0.5);
    assert!(
        peak(&speakers_monitor()) > 0.3,
        "the tone plays on the speakers"
    );

    pulse::mute_speakers().expect("mute");
    pulse::mute_speakers().expect("muting twice is harmless");
    assert_eq!(
        app_stream("sp-test-music").as_deref(),
        Some(pulse::SPEAKERS_SINK)
    );
    let system = peak(&CaptureSource::SystemLoopback(None));
    let pinned = peak(&CaptureSource::SystemLoopback(Some(SPEAKERS.into())));
    let speakers = peak(&speakers_monitor());
    println!("muted: system audio {system}, pinned {pinned}, speakers {speakers}");
    assert!(system > 0.3, "system audio is still captured");
    assert!(
        pinned > 0.3,
        "capture pinned to the muted speakers follows them"
    );
    assert!(speakers < 0.01, "nothing reaches the speakers");
    // SoundPush's device list shows the real speakers as the default, not its own sink.
    let outputs = pulse::list_devices().expect("devices");
    assert!(outputs.iter().all(|d| d.id != "SoundPush Speakers"));

    pulse::unmute_speakers().expect("unmute");
    assert_eq!(app_stream("sp-test-music").as_deref(), Some(SPEAKERS));
    assert!(pactl(&["get-default-sink"]).trim() == SPEAKERS);
    assert_eq!(soundpush_modules(), 0);
    assert!(peak(&speakers_monitor()) > 0.3, "the speakers play again");
}

#[test]
#[ignore = "needs a PulseAudio server"]
fn app_capture_sends_one_app_or_everything_else() {
    fake_speakers();
    let _music = Tone::play("sp-test-music", 0.5);
    let _call = Tone::play("sp-test-call", 0.1);

    let apps = pulse::audio_apps().expect("apps");
    println!("apps: {apps:?}");
    assert!(
        apps.iter()
            .any(|a| a.process == "sp-test-music" && a.active)
    );
    assert!(apps.iter().any(|a| a.process == "sp-test-call"));

    let only = CaptureSource::Application {
        process: "SP-TEST-MUSIC".into(),
        exclude: false,
    };
    let except = CaptureSource::Application {
        process: "sp-test-music".into(),
        exclude: true,
    };
    let (only_peak, except_peak) = (peak(&only), peak(&except));
    println!("only music {only_peak}, all but music {except_peak}");
    assert!(
        (0.4..0.6).contains(&only_peak),
        "only the music is captured"
    );
    assert!(
        (0.05..0.2).contains(&except_peak),
        "only the call is captured"
    );

    // Streams are back on the speakers and SoundPush's modules are gone.
    assert_eq!(app_stream("sp-test-music").as_deref(), Some(SPEAKERS));
    assert_eq!(app_stream("sp-test-call").as_deref(), Some(SPEAKERS));
    assert_eq!(soundpush_modules(), 0);
}

#[test]
#[ignore = "needs a PulseAudio server"]
fn app_capture_keeps_the_app_audible_and_follows_new_streams() {
    fake_speakers();
    let level = Arc::new(Mutex::new(0f32));
    let meter = level.clone();
    let stream = pulse::open_capture(
        &CaptureSource::Application {
            process: "sp-test-late".into(),
            exclude: false,
        },
        2,
        Box::new(move |frames| {
            if let Ok(mut peak) = meter.lock() {
                *peak = frames.iter().fold(*peak, |p, s| p.max(s.abs()));
            }
        }),
        Box::new(|e| panic!("capture failed: {e}")),
    )
    .expect("open capture");
    // The app starts playing after capture started.
    let _late = Tone::play("sp-test-late", 0.5);
    wait_for(|| {
        app_stream("sp-test-late").is_some_and(|s| s.starts_with("soundpush_app_capture_"))
    });
    std::thread::sleep(Duration::from_millis(300));
    *level.lock().unwrap() = 0.0;
    let heard = peak(&speakers_monitor());
    let captured = *level.lock().unwrap();
    println!("late app: captured {captured}, speakers {heard}");
    assert!(captured > 0.3, "the new stream is captured");
    assert!(heard > 0.3, "the loopback keeps it audible");
    drop(stream);
    assert_eq!(app_stream("sp-test-late").as_deref(), Some(SPEAKERS));
    assert_eq!(soundpush_modules(), 0);
}

#[test]
#[ignore = "needs a PulseAudio server"]
fn app_capture_left_by_a_crash_is_removed() {
    fake_speakers();
    let _music = Tone::play("sp-test-music", 0.5);
    // What a SoundPush process that no longer exists left behind.
    let sink = "soundpush_app_capture_999999999_0";
    pactl(&[
        "load-module",
        "module-null-sink",
        &format!("sink_name={sink}"),
    ]);
    pactl(&[
        "load-module",
        "module-loopback",
        &format!("source={sink}.monitor"),
    ]);
    let mut pulse = Pulse::connect().expect("server");
    let input = pulse
        .sink_inputs()
        .expect("inputs")
        .into_iter()
        .find(|i| i.app == "sp-test-music")
        .expect("music");
    pulse.move_sink_input(input.index, sink).expect("move");
    assert_eq!(app_stream("sp-test-music").as_deref(), Some(sink));

    pulse::remove_stale_app_captures().expect("cleanup");
    assert_eq!(app_stream("sp-test-music").as_deref(), Some(SPEAKERS));
    assert_eq!(soundpush_modules(), 0);
}

#[test]
#[ignore = "needs a PulseAudio server"]
fn device_changes_are_reported() {
    fake_speakers();
    let (tx, rx) = mpsc::channel();
    pulse::watch_devices(move |input, output| {
        let _ = tx.send((input, output));
    })
    .expect("watch");
    std::thread::sleep(Duration::from_millis(300));

    let module = pactl(&[
        "load-module",
        "module-null-sink",
        "sink_name=sp_test_headset",
    ]);
    let added = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("device added");
    assert_eq!(added, (false, false));

    pactl(&["set-default-sink", "sp_test_headset"]);
    let switched = std::iter::from_fn(|| rx.recv_timeout(Duration::from_secs(3)).ok())
        .find(|(_, output)| *output);
    assert!(switched.is_some(), "default output change reported");

    pactl(&["unload-module", module.trim()]);
    let removed = std::iter::from_fn(|| rx.recv_timeout(Duration::from_secs(3)).ok()).next();
    assert!(removed.is_some(), "device removal reported");
    pactl(&["set-default-sink", SPEAKERS]);
}
