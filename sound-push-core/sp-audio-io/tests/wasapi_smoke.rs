//! Against the real WASAPI backend on Windows (plan §29.1 "Backend integration"): enumeration, a
//! render stream, and system-audio loopback.
//!
//! Designed to run headless on a CI runner. A Windows Server image has no audio endpoint at all, so
//! every test that needs one reports why it stopped and passes: an absent sound card is the machine's
//! property, not a regression. On a machine with speakers all of it runs for real.
//!
//! ```sh
//! cargo test -p sp-audio-io --test wasapi_smoke -- --nocapture
//! ```
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers fail the test on purpose

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sp_audio_io::cpal_backend::CpalBackend;
use sp_audio_io::{AudioBackend, AudioError, CaptureSource, DeviceKind, RenderTarget};

/// How long a stream is left running before its callbacks are counted.
const RUN: Duration = Duration::from_millis(600);

/// `None` (with a reason on stdout) when this machine has no output endpoint.
fn outputs(backend: &CpalBackend) -> Option<usize> {
    let devices = match backend.list_devices() {
        Ok(devices) => devices,
        Err(e) => {
            println!("skipped: the audio service is unavailable ({e})");
            return None;
        }
    };
    let outputs = devices
        .iter()
        .filter(|d| d.kind == DeviceKind::Output)
        .count();
    if outputs == 0 {
        println!("skipped: this machine has no audio output endpoint");
        return None;
    }
    Some(outputs)
}

#[test]
fn enumeration_answers_without_an_endpoint() {
    let backend = CpalBackend::new();
    assert_eq!(backend.name(), "cpal");
    assert!(
        backend.supports_loopback(),
        "WASAPI can capture what the system plays"
    );
    // The point of the test: enumeration must return a list, even an empty one, never an error.
    let devices = backend.list_devices().expect("enumeration answers");
    for d in &devices {
        assert!(!d.id.is_empty(), "every device has an id");
        assert!(!d.name.is_empty(), "every device has a name: {d:?}");
        assert!(d.channels >= 1, "{d:?}");
        assert!(d.sample_rate >= 8_000, "{d:?}");
    }
    println!("{} devices", devices.len());
}

#[test]
fn render_opens_and_asks_for_audio() {
    let backend = CpalBackend::new();
    if outputs(&backend).is_none() {
        return;
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let stream = backend
        .open_render(
            &RenderTarget::DefaultOutput,
            2,
            Box::new(move |buffer: &mut [f32]| {
                // Silence: a CI machine with a real endpoint should stay quiet.
                buffer.fill(0.0);
                counter.fetch_add(1, Ordering::Relaxed);
            }),
            Box::new(|_| {}),
        )
        .expect("the default output opens");
    let info = stream.info();
    assert!(info.device_sample_rate >= 8_000, "{info:?}");
    assert!(info.channels >= 1, "{info:?}");
    std::thread::sleep(RUN);
    drop(stream);
    assert!(
        calls.load(Ordering::Relaxed) > 0,
        "WASAPI asked for audio at least once in {RUN:?}"
    );
}

#[test]
fn loopback_captures_what_the_system_plays() {
    let backend = CpalBackend::new();
    if outputs(&backend).is_none() {
        return;
    }
    // WASAPI loopback only produces buffers while the endpoint is running, so give it something to
    // play: silence, from the same render path the app uses.
    let keep_alive = backend.open_render(
        &RenderTarget::DefaultOutput,
        2,
        Box::new(|buffer: &mut [f32]| buffer.fill(0.0)),
        Box::new(|_| {}),
    );
    let frames = Arc::new(AtomicUsize::new(0));
    let counter = frames.clone();
    let stream = match backend.open_capture(
        &CaptureSource::SystemLoopback(None),
        2,
        Box::new(move |samples: &[f32]| {
            counter.fetch_add(samples.len(), Ordering::Relaxed);
        }),
        Box::new(|_| {}),
    ) {
        Ok(stream) => stream,
        // A machine with an endpoint but no loopback client (a stripped-down image) is not a failure.
        Err(AudioError::LoopbackUnsupported | AudioError::NoDefaultDevice) => {
            println!("skipped: this machine offers no loopback endpoint");
            return;
        }
        Err(e) => panic!("loopback capture failed: {e}"),
    };
    let info = stream.info();
    assert!(info.device_sample_rate >= 8_000, "{info:?}");
    assert!(info.channels >= 1, "{info:?}");
    std::thread::sleep(RUN);
    drop(stream);
    let was_playing = keep_alive.is_ok();
    drop(keep_alive);
    let samples = frames.load(Ordering::Relaxed);
    println!("loopback delivered {samples} samples");
    // Silence still arrives as samples while the endpoint runs. If the endpoint could not be started
    // at all there is nothing to capture, and that is the machine's property, not a regression.
    if was_playing {
        assert!(samples > 0, "loopback delivered samples in {RUN:?}");
    }
}
