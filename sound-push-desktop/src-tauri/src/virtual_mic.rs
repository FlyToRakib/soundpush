//! The virtual microphone other apps use ("Use phone as computer microphone").
//!
//! - **macOS:** SoundPush's own "SoundPush Microphone", a CoreAudio AudioServerPlugIn embedded in
//!   the app bundle and installed into `/Library/Audio/Plug-Ins/HAL` through macOS's own
//!   administrator prompt (SoundPush never sees the password).
//! - **Windows:** VB-CABLE by VB-Audio (donationware, Microsoft-signed). On request SoundPush
//!   downloads the official, SHA-256-pinned package from vb-audio.com and opens VB-Audio's own
//!   setup (UAC prompt, the user clicks "Install Driver"), then offers a restart, which VB-CABLE
//!   requires. It is not bundled into the SoundPush installer until VB-Audio agrees (see
//!   docs/virtual-microphone.md §4). Our own signed driver replaces it later (plan stage 2).
//! - **Linux:** no driver. SoundPush asks the sound server (PipeWire through pipewire-pulse, or
//!   PulseAudio) for a null sink "SoundPush Microphone Feed" and a source "SoundPush Microphone"
//!   remapped from its monitor. No root, no password.
//!
//! Detection of the device itself lives in the engine hooks (`hooks.rs`, `VIRTUAL_CABLES`).

use serde::Serialize;
use tauri::AppHandle;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// SoundPush can install a virtual microphone on this platform.
    pub supported: bool,
    /// The driver files are installed (the device may still need a restart to appear).
    pub installed: bool,
    /// "soundpush" (our own driver), "vbcable" (VB-Audio's), or "none".
    pub provider: &'static str,
}

pub fn status() -> Status {
    #[cfg(target_os = "macos")]
    return Status {
        supported: true,
        installed: macos::installed(),
        provider: "soundpush",
    };
    #[cfg(target_os = "windows")]
    return Status {
        supported: true,
        installed: windows::installed(),
        provider: "vbcable",
    };
    #[cfg(target_os = "linux")]
    return Status {
        supported: true,
        installed: linux::installed(),
        provider: "soundpush",
    };
    #[allow(unreachable_code)]
    Status {
        supported: false,
        installed: false,
        provider: "none",
    }
}

/// Install the virtual microphone. Blocks while the user answers the OS prompts.
pub fn install(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::install(app);
    #[cfg(target_os = "windows")]
    return {
        let _ = app;
        windows::install()
    };
    #[cfg(target_os = "linux")]
    return {
        let _ = app;
        linux::install()
    };
    #[allow(unreachable_code)]
    {
        let _ = app;
        Err("not available on this platform yet".into())
    }
}

/// Remove SoundPush's own driver. VB-CABLE is left alone: other apps may use it.
pub fn uninstall() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::uninstall();
    #[cfg(target_os = "linux")]
    return linux::uninstall();
    #[allow(unreachable_code)]
    Err("not available on this platform".into())
}

/// Bring back a virtual microphone that lives only as long as the sound server (Linux).
/// Returns at once; the work happens on a background thread.
pub fn restore() {
    #[cfg(target_os = "linux")]
    linux::restore();
}

/// Whether an app is recording from SoundPush Microphone right now (Linux).
#[cfg(target_os = "linux")]
#[allow(dead_code)] // For the engine's "virtual microphone in use" hook, which is wired separately.
pub fn virtual_mic_in_use() -> bool {
    linux::in_use()
}

/// Restart the computer to finish a driver install (Windows, VB-CABLE).
pub fn restart_computer() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return windows::restart();
    #[allow(unreachable_code)]
    Err("not needed on this platform".into())
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tauri::{AppHandle, Manager};

    const DRIVER: &str = "SoundPushMicrophone.driver";
    const BINARY: &str = "Contents/MacOS/SoundPushMicrophone";
    const HAL_DIR: &str = "/Library/Audio/Plug-Ins/HAL";
    /// coreaudiod loads plug-ins when it starts; restarting it makes the device appear now.
    const RESTART_AUDIO: &str = "launchctl kickstart -k system/com.apple.audio.coreaudiod || killall coreaudiod";

    pub fn installed() -> bool {
        Path::new(HAL_DIR).join(DRIVER).join(BINARY).exists()
    }

    /// The driver inside SoundPush.app, or the local build when running `tauri dev`.
    fn bundled(app: &AppHandle) -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(dir) = app.path().resource_dir() {
            candidates.push(dir.join(DRIVER));
        }
        if cfg!(debug_assertions) {
            candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../drivers/macos-virtual-mic/build").join(DRIVER));
        }
        candidates.into_iter().find(|p| p.join(BINARY).exists())
    }

    /// Quote a value for `sh`.
    fn quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// Runs a shell script as root through the standard macOS administrator prompt.
    /// The script is passed as an argument, so no AppleScript string escaping is involved.
    fn run_as_admin(script: &str) -> Result<(), String> {
        let output = Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "do shell script (item 1 of argv) with administrator privileges",
                "-e",
                "end run",
                script,
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        // -128 = the user closed the password prompt.
        if stderr.contains("-128") {
            Err("cancelled".into())
        } else {
            Err(stderr.trim().to_string())
        }
    }

    pub fn install(app: &AppHandle) -> Result<(), String> {
        let source = bundled(app).ok_or("SoundPush Microphone is missing from this app")?;
        let dest = Path::new(HAL_DIR).join(DRIVER);
        let (src, dst) = (quote(&source.to_string_lossy()), quote(&dest.to_string_lossy()));
        run_as_admin(&format!(
            "mkdir -p {hal} && rm -rf {dst} && cp -R {src} {dst} && chown -R root:wheel {dst} && ({RESTART_AUDIO})",
            hal = quote(HAL_DIR)
        ))
    }

    pub fn uninstall() -> Result<(), String> {
        let dst = quote(&Path::new(HAL_DIR).join(DRIVER).to_string_lossy());
        run_as_admin(&format!("rm -rf {dst} && ({RESTART_AUDIO})"))
    }
}

/// The device is two modules on the sound server, loaded by SoundPush (never root):
///
/// 1. `module-null-sink` "SoundPush Microphone Feed": the engine plays the phone microphone into it.
/// 2. `module-remap-source` "SoundPush Microphone" on the sink's monitor: apps record from it.
///
/// Loaded modules last until the sound server stops (logout, reboot, restart). To keep the device:
/// - a marker in SoundPush's data folder makes every SoundPush start load them again (`restore`);
/// - on PipeWire, a pipewire-pulse drop-in (`~/.config/pipewire/pipewire-pulse.conf.d/`) loads
///   them at login too, so apps find their chosen microphone even before SoundPush starts.
///   PulseAudio has no per-user drop-in without replacing `default.pa`, so it relies on the first.
#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;
    use std::time::Duration;

    use sp_audio_io::pulse::{Device, Pulse, SourceOutput};
    use tracing::{info, warn};

    const SINK: &str = "soundpush_microphone_feed";
    const SINK_DESCRIPTION: &str = "SoundPush Microphone Feed";
    const SOURCE: &str = "soundpush_microphone";
    const SOURCE_DESCRIPTION: &str = "SoundPush Microphone";
    /// Sound settings' level meters record from every source; they are not apps using the microphone.
    const LEVEL_METERS: &[&str] = &["org.PulseAudio.pavucontrol", "org.gnome.VolumeControl"];

    fn sink_args() -> String {
        format!("sink_name={SINK} rate=48000 sink_properties=\"device.description='{SINK_DESCRIPTION}'\"")
    }

    fn source_args() -> String {
        format!("master={SINK}.monitor source_name={SOURCE} source_properties=\"device.description='{SOURCE_DESCRIPTION}'\"")
    }

    fn marker() -> PathBuf {
        crate::hooks::data_dir().join("virtual-microphone")
    }

    fn drop_in() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("pipewire/pipewire-pulse.conf.d/soundpush-microphone.conf"))
    }

    fn drop_in_text() -> String {
        let escape = |s: String| s.replace('"', "\\\"");
        format!(
            "# SoundPush Microphone, added by SoundPush (Audio → Install SoundPush Microphone).\n\
             # pipewire-pulse creates it at login. Remove it in SoundPush, or delete this file.\n\
             pulse.cmd = [\n\
             \x20   {{ cmd = \"load-module\" args = \"module-null-sink {}\" flags = [ \"nofail\" ] }}\n\
             \x20   {{ cmd = \"load-module\" args = \"module-remap-source {}\" flags = [ \"nofail\" ] }}\n\
             ]\n",
            escape(sink_args()),
            escape(source_args())
        )
    }

    /// The user installed it; the sound server may still need to load it again (see `restore`).
    pub fn installed() -> bool {
        marker().exists()
    }

    /// Load the modules that are not loaded yet.
    fn ensure_loaded(pulse: &mut Pulse) -> Result<(), String> {
        if !pulse.sinks().map_err(|e| e.to_string())?.iter().any(|d| d.name == SINK) {
            pulse
                .load_module("module-null-sink", &sink_args())
                .map_err(|e| e.to_string())?;
        }
        if !pulse.sources().map_err(|e| e.to_string())?.iter().any(|d| d.name == SOURCE) {
            pulse
                .load_module("module-remap-source", &source_args())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Creates the device now. Running it again re-creates a device the sound server forgot.
    pub fn install() -> Result<(), String> {
        let mut pulse = Pulse::connect().map_err(|e| e.to_string())?;
        ensure_loaded(&mut pulse)?;
        let marker = marker();
        if let Some(dir) = marker.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&marker, b"").map_err(|e| e.to_string())?;
        if pulse.server().is_ok_and(|s| s.is_pipewire())
            && let Some(path) = drop_in()
        {
            let written = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, drop_in_text()));
            // The device works without it; it only would not come back before SoundPush starts.
            if let Err(e) = written {
                warn!(error = %e, path = %path.display(), "could not save the PipeWire configuration");
            }
        }
        Ok(())
    }

    pub fn uninstall() -> Result<(), String> {
        for path in [Some(marker()), drop_in()].into_iter().flatten() {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {e}", path.display())),
                _ => {}
            }
        }
        // Without a sound server nothing is loaded.
        let Ok(mut pulse) = Pulse::connect() else {
            return Ok(());
        };
        let modules = pulse.modules().map_err(|e| e.to_string())?;
        // The source reads the sink's monitor, so it goes first.
        let ours = [
            ("module-remap-source", format!("source_name={SOURCE}")),
            ("module-null-sink", format!("sink_name={SINK}")),
        ];
        for (name, pair) in ours {
            for module in modules.iter().filter(|m| m.name == name && has_argument(&m.argument, &pair)) {
                pulse.unload_module(module.index).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub fn restore() {
        if !installed() {
            return;
        }
        let spawned = std::thread::Builder::new().name("sp-virtual-mic".into()).spawn(|| {
            // At login SoundPush may start before the sound server does.
            for _ in 0..30 {
                if let Ok(mut pulse) = Pulse::connect() {
                    match ensure_loaded(&mut pulse) {
                        Ok(()) => info!("SoundPush Microphone is available"),
                        Err(e) => warn!(error = %e, "could not restore SoundPush Microphone"),
                    }
                    return;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            warn!("no sound server; SoundPush Microphone was not restored");
        });
        if let Err(e) = spawned {
            warn!(error = %e, "could not restore SoundPush Microphone");
        }
    }

    pub fn in_use() -> bool {
        let Ok(mut pulse) = Pulse::connect() else {
            return false;
        };
        match (pulse.sources(), pulse.source_outputs()) {
            (Ok(sources), Ok(outputs)) => recording_from(SOURCE, &sources, &outputs),
            _ => false,
        }
    }

    /// Whether an app (not a level meter) is recording, unpaused, from the source named `source`.
    fn recording_from(source: &str, sources: &[Device], outputs: &[SourceOutput]) -> bool {
        let Some(index) = sources.iter().find(|d| d.name == source).map(|d| d.index) else {
            return false;
        };
        outputs
            .iter()
            .any(|o| o.source == index && !o.corked && !LEVEL_METERS.contains(&o.application_id.as_str()))
    }

    /// Whether a module argument string contains `pair` ("key=value") as a whole word.
    fn has_argument(argument: &str, pair: &str) -> bool {
        argument.split_whitespace().any(|word| word == pair)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn source(name: &str, index: u32) -> Device {
            Device {
                name: name.into(),
                description: String::new(),
                index,
                channels: 2,
                sample_rate: 48_000,
                monitor: None,
                is_monitor: false,
            }
        }

        fn output(source: u32, corked: bool, application_id: &str) -> SourceOutput {
            SourceOutput {
                source,
                corked,
                application_id: application_id.into(),
            }
        }

        #[test]
        fn in_use_only_when_an_app_records_from_our_source() {
            let sources = [source("alsa_input.usb-mic", 3), source(SOURCE, 7)];
            assert!(recording_from(SOURCE, &sources, &[output(7, false, "com.discordapp.Discord")]));
            assert!(recording_from(SOURCE, &sources, &[output(3, false, ""), output(7, false, "")]));
            // Another microphone, a paused stream, or a level meter.
            assert!(!recording_from(SOURCE, &sources, &[output(3, false, "com.discordapp.Discord")]));
            assert!(!recording_from(SOURCE, &sources, &[output(7, true, "com.discordapp.Discord")]));
            assert!(!recording_from(SOURCE, &sources, &[output(7, false, "org.PulseAudio.pavucontrol")]));
            // Not installed.
            assert!(!recording_from(SOURCE, &sources[..1], &[output(7, false, "")]));
        }

        #[test]
        fn modules_are_matched_by_whole_arguments() {
            assert!(has_argument(&sink_args(), "sink_name=soundpush_microphone_feed"));
            assert!(has_argument(&source_args(), "source_name=soundpush_microphone"));
            assert!(!has_argument("source_name=soundpush_microphone_2", "source_name=soundpush_microphone"));
        }

        #[test]
        fn drop_in_escapes_quotes_inside_args() {
            let text = drop_in_text();
            assert!(text.contains(r#"args = "module-null-sink sink_name=soundpush_microphone_feed rate=48000 sink_properties=\"device.description='SoundPush Microphone Feed'\"""#));
            assert!(text.contains("module-remap-source master=soundpush_microphone_feed.monitor"));
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    /// Official VB-CABLE package. Keep URL and checksum in sync with `drivers/vbcable/fetch.sh`.
    const PACKAGE_URL: &str = "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip";
    const PACKAGE_SHA256: &str = "B950E39F01AF1D04EA623C8F6D8EB9B6EA5C477C637295FABF20631C85116BFB";
    /// Run helper processes without flashing a console window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// VB-CABLE's driver file (`vbaudio_cable64_win10.sys`, or the ARM64/older variants).
    pub fn installed() -> bool {
        let drivers = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join(r"System32\drivers");
        std::fs::read_dir(drivers)
            .map(|entries| {
                entries
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with("vbaudio_cable"))
            })
            .unwrap_or(false)
    }

    /// Download the package, verify it, and open VB-Audio's setup elevated (UAC).
    /// `Start-Process -Wait` returns when the user closes VB-Audio's setup window.
    pub fn install() -> Result<(), String> {
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$dir = Join-Path $env:TEMP 'SoundPush-VBCABLE'
Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $dir | Out-Null
$zip = Join-Path $dir 'VBCABLE_Driver_Pack.zip'
Invoke-WebRequest -Uri '{PACKAGE_URL}' -OutFile $zip -UseBasicParsing
if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -ne '{PACKAGE_SHA256}') {{ throw 'The VB-CABLE download did not match the expected checksum.' }}
Expand-Archive -LiteralPath $zip -DestinationPath $dir -Force
$setup = if ($env:PROCESSOR_ARCHITECTURE -eq 'x86') {{ 'VBCABLE_Setup.exe' }} else {{ 'VBCABLE_Setup_x64.exe' }}
Start-Process -FilePath (Join-Path $dir $setup) -WorkingDirectory $dir -Verb RunAs -Wait
Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
"#
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            // The setup window closed. Only the driver file proves "Install Driver" was clicked;
            // closing the window without it is a choice, reported like a declined prompt.
            return if installed() { Ok(()) } else { Err("cancelled".into()) };
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Declining the UAC prompt: "The operation was canceled by the user."
        if stderr.contains("canceled by the user") {
            return Err("cancelled".into());
        }
        Err(stderr
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("VB-CABLE setup failed")
            .to_string())
    }

    /// Restart in 10 seconds so the user sees Windows' own notice first.
    pub fn restart() -> Result<(), String> {
        let status = Command::new("shutdown.exe")
            .args(["/r", "/t", "10", "/c", "SoundPush: restarting to finish installing VB-CABLE"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err("Windows did not accept the restart request".into()) }
    }
}
