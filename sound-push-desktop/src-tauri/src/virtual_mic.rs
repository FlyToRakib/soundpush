//! SoundPush Microphone: the app's own virtual microphone driver.
//!
//! macOS: a CoreAudio AudioServerPlugIn embedded in the app bundle and installed into
//! `/Library/Audio/Plug-Ins/HAL` on request, through macOS's own administrator prompt
//! (SoundPush never sees the password).

use serde::Serialize;
use tauri::AppHandle;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// This platform has a built-in SoundPush Microphone.
    pub supported: bool,
    pub installed: bool,
}

pub fn status() -> Status {
    Status {
        supported: cfg!(target_os = "macos"),
        installed: installed(),
    }
}

#[cfg(target_os = "macos")]
fn installed() -> bool {
    imp::installed()
}

#[cfg(not(target_os = "macos"))]
fn installed() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn install(app: &AppHandle) -> Result<(), String> {
    imp::install(app)
}

#[cfg(not(target_os = "macos"))]
pub fn install(_app: &AppHandle) -> Result<(), String> {
    Err("not available on this platform yet".into())
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<(), String> {
    imp::uninstall()
}

#[cfg(not(target_os = "macos"))]
pub fn uninstall() -> Result<(), String> {
    Err("not available on this platform yet".into())
}

#[cfg(target_os = "macos")]
mod imp {
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

    fn bundled(app: &AppHandle) -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(dir) = app.path().resource_dir() {
            candidates.push(dir.join(DRIVER));
        }
        if cfg!(debug_assertions) {
            // `tauri dev` runs without a bundle.
            candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../drivers/macos-virtual-mic/build").join(DRIVER));
        }
        candidates.into_iter().find(|p| p.join(BINARY).exists())
    }

    fn quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// Runs a shell script as root through the standard macOS administrator prompt.
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
