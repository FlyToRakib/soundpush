//! USB connection for Android phones through adb (plan §16.1, §17.1; ADR-0005).
//!
//! `adb reverse tcp:<phone port> tcp:<engine TCP port>` forwards the phone's loopback port to
//! the engine's loopback TLS-over-TCP listener; the phone's engine dials it whenever Wi-Fi paths
//! fail (or first, when it has none). The forward carries the same mutually authenticated TLS as
//! any other connection, so nothing here affects identity or trust.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

const ADB_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbDevice {
    pub serial: String,
    pub model: String,
    /// False until the user allows USB debugging on the phone.
    pub authorized: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbStatus {
    pub adb_found: bool,
    pub devices: Vec<UsbDevice>,
    /// The engine's loopback TCP port (0 when it could not listen).
    pub tcp_port: u16,
}

/// Where adb usually lives, most specific first; plain `adb` (PATH) last.
fn adb_candidates() -> Vec<PathBuf> {
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
    let mut dirs: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("platform-tools"))
        .collect();
    if let Some(local) = dirs::data_local_dir() {
        dirs.push(local.join("Android").join("Sdk").join("platform-tools"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Library").join("Android").join("sdk").join("platform-tools"));
        dirs.push(home.join("Android").join("Sdk").join("platform-tools"));
    }
    let mut candidates: Vec<PathBuf> = dirs.into_iter().map(|d| d.join(exe)).filter(|p| p.is_file()).collect();
    candidates.push(PathBuf::from(exe));
    candidates
}

fn find_adb() -> Option<PathBuf> {
    adb_candidates()
        .into_iter()
        .find(|adb| run(adb, &["version"]).is_ok_and(|o| o.status.success()))
}

/// Run adb with a time limit and no console window.
fn run(adb: &PathBuf, args: &[&str]) -> Result<Output, String> {
    let mut command = Command::new(adb);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + ADB_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("adb did not respond".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Parse `adb devices -l`.
pub fn parse_devices(output: &str) -> Vec<UsbDevice> {
    output
        .lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            let state = parts.next()?;
            if !valid_serial(serial) || !matches!(state, "device" | "unauthorized") {
                return None;
            }
            let model = parts
                .find_map(|p| p.strip_prefix("model:"))
                .map(|m| m.replace('_', " "))
                .unwrap_or_else(|| serial.to_string());
            Some(UsbDevice {
                serial: serial.to_string(),
                model,
                authorized: state == "device",
            })
        })
        .collect()
}

fn valid_serial(serial: &str) -> bool {
    !serial.is_empty()
        && serial.len() <= 128
        && serial
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
}

pub fn status(tcp_port: u16) -> UsbStatus {
    let Some(adb) = find_adb() else {
        return UsbStatus {
            adb_found: false,
            devices: Vec::new(),
            tcp_port,
        };
    };
    let devices = run(&adb, &["devices", "-l"])
        .map(|o| parse_devices(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    UsbStatus {
        adb_found: true,
        devices,
        tcp_port,
    }
}

/// Forward `phone_port` on the phone to the engine's TCP port on this computer.
pub fn connect(serial: &str, phone_port: u16, tcp_port: u16) -> Result<(), String> {
    if !valid_serial(serial) {
        return Err("invalid device".into());
    }
    if tcp_port == 0 || phone_port == 0 {
        return Err("USB connections are unavailable (no TCP listener)".into());
    }
    let adb = find_adb().ok_or("adb not found")?;
    let local = format!("tcp:{phone_port}");
    let remote = format!("tcp:{tcp_port}");
    let output = run(&adb, &["-s", serial, "reverse", &local, &remote])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr)
            .trim()
            .chars()
            .take(200)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adb_device_list() {
        let out = "* daemon started successfully\nList of devices attached\n\
                   8f3a1c2b       device usb:1-1 product:joyeuse model:Redmi_Note_9_Pro device:joyeuse transport_id:3\n\
                   emulator-5554  unauthorized transport_id:4\n\
                   bad;serial     device\n\
                   0123456789     offline\n\n";
        let devices = parse_devices(out);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].model, "Redmi Note 9 Pro");
        assert!(devices[0].authorized);
        assert_eq!(devices[1].serial, "emulator-5554");
        assert!(!devices[1].authorized);
        assert!(!valid_serial("a b") && !valid_serial("") && valid_serial("192.168.1.2:5555"));
    }
}
