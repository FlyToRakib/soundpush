fn main() {
    build_macos_virtual_mic();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // SoundPush runs on macOS 13, but Core Audio's process tap functions exist from 14.2.
        // Weak references let the app start there; they are only called when available
        // (`sp_audio_io::macos_sck::process_taps_supported`).
        println!("cargo:rustc-link-arg-bins=-Wl,-weak_framework,CoreAudio");
    }
    tauri_build::build();
}

/// Builds SoundPush Microphone (the app's own virtual microphone driver) so the macOS
/// app bundle can embed it. Runs before `tauri_build`, which checks bundle resources exist.
fn build_macos_virtual_mic() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../drivers/macos-virtual-mic");
    for file in ["SoundPushMicrophone.c", "Info.plist", "build.sh"] {
        println!("cargo:rerun-if-changed={}", dir.join(file).display());
    }
    let status = std::process::Command::new("sh")
        .arg(dir.join("build.sh"))
        .status()
        .unwrap_or_else(|e| panic!("could not run the virtual microphone build: {e}"));
    assert!(
        status.success(),
        "building SoundPushMicrophone.driver failed"
    );
}
