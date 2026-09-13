//! Platform integration points implemented by each app shell.

use std::path::PathBuf;
use std::sync::Arc;

use sp_audio_io::{AudioBackend, CaptureSource, RenderTarget};

/// Why the engine needs the OS to keep it alive (Android foreground-service types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeepAlive {
    pub playback: bool,
    pub microphone: bool,
    pub app_audio_capture: bool,
}

impl KeepAlive {
    pub fn any(self) -> bool {
        self.playback || self.microphone || self.app_audio_capture
    }
}

pub trait PlatformHooks: Send + Sync + 'static {
    /// Directory for settings, trust store and identity.
    fn data_dir(&self) -> PathBuf;

    /// 32-byte key protecting the identity and trust store at rest, provided by
    /// the OS keystore (DPAPI, Keychain, Secret Service, Android Keystore).
    fn storage_key(&self) -> [u8; 32];

    /// "windows", "linux", "macos", "android", "ios".
    fn platform(&self) -> &'static str;

    /// Suggested device name on first run (computer or phone model name).
    fn default_device_name(&self) -> String;

    fn audio_backend(&self) -> Arc<dyn AudioBackend>;

    /// Capture source for this device's "apps" endpoint (Android playback capture).
    fn app_audio_source(&self) -> Option<CaptureSource> {
        None
    }

    /// Render target that feeds the virtual microphone, if one is installed.
    fn virtual_mic_target(&self, configured: Option<&str>) -> Option<RenderTarget> {
        configured.map(|d| RenderTarget::Output(d.to_string()))
    }

    /// When `output` is the playback side of a known virtual cable, the name other apps
    /// select as a microphone (its recording side). `None` for regular speakers.
    fn virtual_cable_input(&self, _output: &str) -> Option<String> {
        None
    }

    /// The OS audio device list may have changed; drop cached device detection.
    fn audio_devices_changed(&self) {}

    /// Mute or unmute this device's physical speakers ("Mute PC"). Returns false if unsupported.
    fn set_speakers_muted(&self, _muted: bool) -> bool {
        false
    }

    /// Whether the OS currently grants microphone access.
    fn microphone_permitted(&self) -> bool {
        true
    }

    /// Ask the OS to keep the engine alive for these activities (Android FGS types).
    fn keep_alive(&self, _reason: KeepAlive) {}

    /// Notify the shell that a user-visible prompt needs attention while backgrounded.
    fn attention_needed(&self, _title_key: &str, _peer_name: &str) {}
}
