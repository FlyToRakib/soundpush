//! Desktop implementation of the engine's platform hooks.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::RngCore;
use sp_audio_io::cpal_backend::CpalBackend;
use sp_audio_io::{AudioBackend, DeviceKind, RenderTarget};
use sp_engine::{KeepAlive, PlatformHooks};
use tauri::{AppHandle, Manager, UserAttentionType};
use tracing::warn;

use crate::power::SleepInhibitor;

/// Playback devices that feed a virtual microphone, in preference order.
const VIRTUAL_MIC_CANDIDATES: &[&str] = &[
    "SoundPush Microphone",
    "CABLE Input",
    "VB-Audio Virtual Cable",
    "BlackHole 2ch",
    "BlackHole",
    "Voicemeeter Input",
];

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("SoundPush")
}

pub struct DesktopHooks {
    app: AppHandle,
    data_dir: PathBuf,
    backend: Arc<CpalBackend>,
    virtual_mic_cache: Mutex<(Option<Instant>, Option<String>)>,
    inhibitor: Mutex<Option<SleepInhibitor>>,
}

impl DesktopHooks {
    pub fn new(app: AppHandle, data_dir: PathBuf) -> Self {
        Self {
            app,
            data_dir,
            backend: Arc::new(CpalBackend::new()),
            virtual_mic_cache: Mutex::new((None, None)),
            inhibitor: Mutex::new(None),
        }
    }

    pub fn set_prevent_sleep(&self, prevent: bool) {
        if let Ok(mut guard) = self.inhibitor.lock() {
            *guard = if prevent { SleepInhibitor::acquire() } else { None };
        }
    }

    fn detect_virtual_mic(&self) -> Option<String> {
        let mut cache = self.virtual_mic_cache.lock().ok()?;
        let fresh = cache.0.is_some_and(|t| t.elapsed() < Duration::from_secs(10));
        if !fresh {
            let outputs: Vec<String> = self
                .backend
                .list_devices()
                .unwrap_or_default()
                .into_iter()
                .filter(|d| d.kind == DeviceKind::Output)
                .map(|d| d.name)
                .collect();
            let found = VIRTUAL_MIC_CANDIDATES
                .iter()
                .find_map(|c| outputs.iter().find(|o| o.contains(c)).cloned());
            *cache = (Some(Instant::now()), found);
        }
        cache.1.clone()
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
        configured
            .map(str::to_string)
            .or_else(|| self.detect_virtual_mic())
            .map(RenderTarget::Output)
    }

    fn set_speakers_muted(&self, muted: bool) -> bool {
        crate::power::set_default_output_muted(muted)
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
