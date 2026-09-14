//! Diagnostic: what does cpal see while a system-audio tap exists?
//! `cargo test -p sp-audio-io --test macos_tap_diagnostics -- --ignored --nocapture`
#![cfg(target_os = "macos")]

use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait};
use sp_audio_io::macos_tap::SystemTap;

fn dump(label: &str) {
    let host = cpal::default_host();
    println!("== {label}");
    for device in host.devices().expect("devices") {
        let name = device
            .name()
            .unwrap_or_else(|e| format!("<name error {e}>"));
        let input = device
            .default_input_config()
            .map(|c| format!("{c:?}"))
            .unwrap_or_else(|e| format!("none ({e})"));
        let output = device
            .default_output_config()
            .map(|c| format!("{} ch", c.channels()))
            .unwrap_or_else(|_| "none".into());
        println!("  {name:?}\n    input: {input}\n    output: {output}");
    }
    let inputs: Vec<String> = host
        .input_devices()
        .expect("inputs")
        .filter_map(|d| d.name().ok())
        .collect();
    println!("  input_devices(): {inputs:?}");
}

#[test]
#[ignore = "needs Core Audio"]
fn list_devices_with_tap() {
    dump("before tap");
    let tap = SystemTap::create().expect("tap created");
    for wait in [0u64, 500, 1500] {
        std::thread::sleep(Duration::from_millis(wait));
        dump(&format!("with tap (+{wait} ms)"));
    }
    drop(tap);
}
