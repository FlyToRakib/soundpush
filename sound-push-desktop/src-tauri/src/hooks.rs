//! Desktop implementation of the engine's platform hooks.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::RngCore;
use sp_audio_io::cpal_backend::CpalBackend;
use sp_audio_io::{AudioBackend, RenderTarget};
use sp_engine::{KeepAlive, PlatformHooks};
use tauri::{AppHandle, Manager, UserAttentionType};
use tracing::warn;

use crate::power::SleepInhibitor;

/// Virtual cables in preference order: (part of the playback device name, recording-side
/// name that apps select as a microphone). `None` when both sides share the device name.
const VIRTUAL_CABLES: &[(&str, Option<&str>)] = &[
    ("SoundPush Microphone", None),
    ("CABLE Input", Some("CABLE Output (VB-Audio Virtual Cable)")),
    ("Hi-Fi Cable Input", Some("Hi-Fi Cable Output (VB-Audio Hi-Fi Cable)")),
    ("BlackHole", None),
    ("Voicemeeter Input", Some("Voicemeeter Out B1 (VB-Audio Voicemeeter VAIO)")),
    ("Voicemeeter AUX Input", Some("Voicemeeter Out B2 (VB-Audio Voicemeeter AUX VAIO)")),
    ("Loopback Audio", None),
];

/// Recording-side name for a virtual cable's playback device; `None` for real speakers.
fn cable_input_name(output: &str) -> Option<String> {
    VIRTUAL_CABLES
        .iter()
        .find(|(playback, _)| output.contains(playback))
        .map(|(_, recording)| recording.map_or_else(|| output.to_string(), str::to_string))
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("SoundPush")
}

/// Exists while SoundPush has muted the speakers, so a crash can be undone on the next start.
const MUTED_MARKER: &str = "speakers-muted";

pub struct DesktopHooks {
    app: AppHandle,
    data_dir: PathBuf,
    backend: Arc<CpalBackend>,
    /// Playback device names and when they were listed.
    outputs_cache: Mutex<(Option<Instant>, Vec<String>)>,
    inhibitor: Mutex<Option<SleepInhibitor>>,
    /// Last firewall and network profile check (`network.rs`).
    network: Mutex<crate::network::NetworkStatus>,
    inbound_blocked: AtomicBool,
}

impl DesktopHooks {
    pub fn new(app: AppHandle, data_dir: PathBuf) -> Self {
        let marker = data_dir.join(MUTED_MARKER);
        if marker.exists() {
            // The previous run muted the speakers to send this computer's audio and never came back.
            warn!("speakers were left muted by the previous run; unmuting");
            if crate::power::set_default_output_muted(false) {
                let _ = std::fs::remove_file(&marker);
            }
        }
        Self {
            app,
            data_dir,
            backend: Arc::new(CpalBackend::new()),
            outputs_cache: Mutex::new((None, Vec::new())),
            inhibitor: Mutex::new(None),
            network: Mutex::new(crate::network::NetworkStatus::default()),
            inbound_blocked: AtomicBool::new(false),
        }
    }

    pub fn backend(&self) -> Arc<CpalBackend> {
        self.backend.clone()
    }

    pub fn set_network_status(&self, status: crate::network::NetworkStatus) {
        self.inbound_blocked
            .store(status.firewall_enabled && status.blocked, Ordering::Relaxed);
        if let Ok(mut current) = self.network.lock() {
            *current = status;
        }
    }

    pub fn network_status(&self) -> crate::network::NetworkStatus {
        self.network.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn set_prevent_sleep(&self, prevent: bool) {
        if let Ok(mut guard) = self.inhibitor.lock() {
            *guard = if prevent { SleepInhibitor::acquire() } else { None };
        }
    }

    /// Playback device names, listed at most every 10 s (enumeration is slow on some drivers).
    /// Only outputs are listed: touching microphones here would trigger macOS's microphone
    /// permission prompt on every refresh.
    fn output_names(&self) -> Vec<String> {
        let Ok(mut cache) = self.outputs_cache.lock() else {
            return Vec::new();
        };
        let fresh = cache.0.is_some_and(|t| t.elapsed() < Duration::from_secs(10));
        if !fresh {
            *cache = (Some(Instant::now()), self.backend.output_device_names());
        }
        cache.1.clone()
    }

    fn detect_virtual_mic(&self) -> Option<String> {
        let outputs = self.output_names();
        VIRTUAL_CABLES
            .iter()
            .find_map(|(playback, _)| outputs.iter().find(|o| o.contains(playback)).cloned())
    }
}

impl PlatformHooks for DesktopHooks {
    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn storage_key(&self) -> [u8; 32] {
        storage_key(&self.data_dir)
    }

    fn platform(&self) -> &'static str {
        if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        }
    }

    fn default_device_name(&self) -> String {
        let name = gethostname::gethostname().to_string_lossy().to_string();
        let name = name.trim_end_matches(".local").trim();
        if name.is_empty() { "My computer".into() } else { name.chars().take(64).collect() }
    }

    fn audio_backend(&self) -> Arc<dyn AudioBackend> {
        self.backend.clone()
    }

    fn virtual_mic_target(&self, configured: Option<&str>) -> Option<RenderTarget> {
        // A chosen device counts only if it is a virtual cable that is present: feeding the
        // phone's microphone into real speakers plays the user's voice out loud and no app can
        // record it, and a cable that was removed would make every microphone route fail.
        let outputs = self.output_names();
        configured
            .filter(|d| cable_input_name(d).is_some() && outputs.iter().any(|o| o == d))
            .map(str::to_string)
            .or_else(|| self.detect_virtual_mic())
            .map(RenderTarget::Output)
    }

    fn virtual_cable_input(&self, output: &str) -> Option<String> {
        cable_input_name(output)
    }

    fn audio_devices_changed(&self) {
        if let Ok(mut cache) = self.outputs_cache.lock() {
            cache.0 = None;
        }
    }

    fn set_speakers_muted(&self, muted: bool) -> bool {
        let ok = crate::power::set_default_output_muted(muted);
        if ok {
            // Remember a mute across a crash so the next start can undo it.
            let marker = self.data_dir.join(MUTED_MARKER);
            if muted {
                let _ = std::fs::write(&marker, b"");
            } else {
                let _ = std::fs::remove_file(&marker);
            }
        }
        ok
    }

    fn microphone_permitted(&self) -> bool {
        !matches!(crate::system::microphone(), "denied" | "restricted")
    }

    fn virtual_mic_in_use(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            // Linux integration: return `crate::virtual_mic::linux::virtual_mic_in_use()` here.
            false
        }
        #[cfg(not(target_os = "linux"))]
        {
            // The recording side of every virtual cable that is present.
            let outputs = self.output_names();
            VIRTUAL_CABLES
                .iter()
                .filter_map(|(playback, recording)| {
                    let output = outputs.iter().find(|o| o.contains(playback))?;
                    Some(recording.map_or_else(|| output.clone(), str::to_string))
                })
                .any(|input| crate::system::capture_device_in_use(&input))
        }
    }

    fn inbound_blocked(&self) -> bool {
        self.inbound_blocked.load(Ordering::Relaxed)
    }

    fn keep_alive(&self, _reason: KeepAlive) {
        // Desktop apps are not suspended by the OS; sleep prevention is driven by settings.
    }

    fn attention_needed(&self, _title_key: &str, _peer_name: &str) {
        crate::show_main_window(&self.app);
        if let Some(w) = self.app.get_webview_window(crate::MAIN_WINDOW) {
            let _ = w.request_user_attention(Some(UserAttentionType::Informational));
        }
    }
}

/// 32-byte storage key from the OS keystore, with a protected-file fallback.
fn storage_key(dir: &Path) -> [u8; 32] {
    const SERVICE: &str = "SoundPush";
    const ACCOUNT: &str = "storage-key";

    let decode = |hex: &str| -> Option<[u8; 32]> {
        let bytes: Option<Vec<u8>> = (0..hex.len())
            .step_by(2)
            .map(|i| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
            .collect();
        bytes?.try_into().ok()
    };
    let encode = |key: &[u8; 32]| key.iter().map(|b| format!("{b:02x}")).collect::<String>();

    let fallback = dir.join("storage.key");

    // Development builds are re-signed on every build, which would make macOS show a Keychain
    // prompt after each rebuild. They keep the key in a user-only file (migrated once from the Keychain).
    if cfg!(debug_assertions) {
        if let Some(key) = std::fs::read_to_string(&fallback).ok().and_then(|s| decode(s.trim())) {
            return key;
        }
        let migrated = keyring::Entry::new(SERVICE, ACCOUNT)
            .ok()
            .and_then(|e| e.get_password().ok())
            .and_then(|s| decode(&s));
        if let Some(key) = migrated {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(&fallback, encode(&key));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&fallback, std::fs::Permissions::from_mode(0o600));
            }
            return key;
        }
    }

    if !cfg!(debug_assertions)
        && let Ok(entry) = keyring::Entry::new(SERVICE, ACCOUNT)
    {
        if let Ok(secret) = entry.get_password() {
            if let Some(key) = decode(&secret) {
                return key;
            }
        }
        // Migrate a key created by the file fallback so existing data stays readable.
        let key = std::fs::read_to_string(&fallback)
            .ok()
            .and_then(|s| decode(s.trim()))
            .unwrap_or_else(random_key);
        if entry.set_password(&encode(&key)).is_ok() {
            let _ = std::fs::remove_file(&fallback);
            return key;
        }
        warn!("OS keystore unavailable; using protected key file");
    }

    if let Some(key) = std::fs::read_to_string(&fallback).ok().and_then(|s| decode(s.trim())) {
        return key;
    }
    let key = random_key();
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&fallback, encode(&key));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&fallback, std::fs::Permissions::from_mode(0o600));
    }
    key
}

fn random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    key
}

#[cfg(test)]
mod tests {
    use super::cable_input_name;

    #[test]
    fn only_virtual_cables_feed_the_virtual_microphone() {
        assert_eq!(
            cable_input_name("CABLE Input (VB-Audio Virtual Cable)").as_deref(),
            Some("CABLE Output (VB-Audio Virtual Cable)")
        );
        assert_eq!(cable_input_name("BlackHole 2ch").as_deref(), Some("BlackHole 2ch"));
        assert_eq!(cable_input_name("MacBook Air Speakers"), None);
        assert_eq!(cable_input_name("BenQ EW3270U (NVIDIA High Definition Audio)"), None);
    }
}
