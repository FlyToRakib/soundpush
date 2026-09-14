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
    #[allow(unreachable_code)]
    Err("not available on this platform".into())
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
