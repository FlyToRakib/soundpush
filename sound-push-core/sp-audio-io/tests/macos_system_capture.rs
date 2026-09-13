//! Manual hardware test: captures real system audio through the Core Audio tap.
//! Run with sound playing: `cargo test -p sp-audio-io --test macos_system_capture -- --ignored --nocapture`
#![cfg(target_os = "macos")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sp_audio_io::cpal_backend::CpalBackend;
use sp_audio_io::{AudioBackend, CaptureSource};

#[test]
#[ignore = "needs audio hardware and the system-audio recording permission"]
fn captures_system_audio() {
    let backend = CpalBackend::new();
    assert!(backend.supports_loopback());

    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = samples.clone();
    let stream = backend
        .open_capture(
            &CaptureSource::SystemLoopback(None),
            2,
            Box::new(move |s: &[f32]| sink.lock().unwrap().extend_from_slice(s)),
            Box::new(|e| eprintln!("capture error: {e}")),
        )
        .expect("system capture opens");
    println!("stream: {:?}", stream.info());

    // Play sounds only after capture is running, so they fall inside the recording window.
    let player = std::process::Command::new("sh")
        .args(["-c", "for s in Submarine Glass Hero Funk; do afplay -v 1 /System/Library/Sounds/$s.aiff; done"])
        .spawn();

    std::thread::sleep(Duration::from_millis(3000));
    drop(stream);
    if let Ok(mut p) = player {
        let _ = p.kill();
    }

    let captured = samples.lock().unwrap();
    let peak = captured.iter().fold(0f32, |m, s| m.max(s.abs()));
    println!("captured {} samples, peak {peak:.3}", captured.len());
    assert!(captured.len() > 48_000, "stream delivered audio frames");
}
