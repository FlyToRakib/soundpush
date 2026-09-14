//! Persistent settings (one schema for every platform).
//!
//! Stored as JSON with atomic writes. Unknown fields are ignored and missing
//! fields take defaults, so older and newer versions can read each other's files.
//! A corrupted file is moved aside and defaults are used.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sp_media::profile::{LatencyProfile, Quality};
use sp_security::secretbox::write_atomic;

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Visibility {
    Everyone,
    #[default]
    TrustedOnly,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LatencyMode {
    LowLatency,
    #[default]
    Balanced,
    Stable,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum QualityMode {
    #[default]
    Auto,
    Opus,
    Lossless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AudioFocusMode {
    /// Pause when another app plays; resume afterwards.
    #[default]
    Pause,
    /// Lower SoundPush volume while another app plays.
    Duck,
    /// Keep playing alongside other apps.
    Mix,
    /// Keep playing, including during phone calls (not guaranteed on every device).
    MixDuringCalls,
}

/// Android microphone input presets (AudioRelay's seven modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum MicMode {
    Default,
    #[default]
    VoiceCommunication,
    Raw,
    VoicePerformance,
    VoiceRecognition,
    Camcorder,
    Mic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StreamSettings {
    pub latency: LatencyMode,
    pub custom_min_ms: u32,
    pub custom_max_ms: u32,
    pub quality: QualityMode,
    /// Used when `quality == Opus`.
    pub opus_bitrate: u32,
    pub redundancy: bool,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            latency: LatencyMode::Balanced,
            custom_min_ms: 30,
            custom_max_ms: 120,
            quality: QualityMode::Auto,
            opus_bitrate: 128_000,
            redundancy: false,
        }
    }
}

impl StreamSettings {
    pub fn latency_profile(&self) -> LatencyProfile {
        match self.latency {
            LatencyMode::LowLatency => LatencyProfile::LowLatency,
            LatencyMode::Balanced => LatencyProfile::Balanced,
            LatencyMode::Stable => LatencyProfile::Stable,
            LatencyMode::Custom => LatencyProfile::Custom {
                min_ms: self.custom_min_ms,
                max_ms: self.custom_max_ms,
            },
        }
    }

    pub fn quality(&self) -> Quality {
        match self.quality {
            QualityMode::Auto => Quality::Auto,
            QualityMode::Opus => Quality::Opus(self.opus_bitrate),
            QualityMode::Lossless => Quality::Lossless,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OutputSettings {
    /// `None` follows the system default output.
    pub device: Option<String>,
    pub volume: f32,
    pub balance: f32,
    pub mono: bool,
    /// Positive values delay audio (lip-sync adjustment).
    pub av_offset_ms: i32,
    pub compatibility_output: bool,
    pub output_effects: bool,
    pub pause_on_headset_disconnect: bool,
    pub audio_focus: AudioFocusMode,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            device: None,
            volume: 1.0,
            balance: 0.0,
            mono: false,
            av_offset_ms: 0,
            compatibility_output: false,
            output_effects: false,
            pause_on_headset_disconnect: true,
            audio_focus: AudioFocusMode::Pause,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MicSettings {
    /// `None` uses the system default input.
    pub device: Option<String>,
    pub gain_db: f32,
    pub noise_suppression: bool,
    pub mode: MicMode,
    pub system_agc: bool,
    pub system_noise_suppression: bool,
    pub system_echo_cancellation: bool,
    pub monitor: bool,
}

impl Default for MicSettings {
    fn default() -> Self {
        Self {
            device: None,
            gain_db: 0.0,
            noise_suppression: false,
            mode: MicMode::VoiceCommunication,
            system_agc: false,
            system_noise_suppression: true,
            system_echo_cancellation: true,
            monitor: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CaptureSettings {
    /// Output device whose audio is sent (`None` = system default).
    pub system_device: Option<String>,
    /// Keep the stream at full level while muting local speakers.
    pub mute_local_speakers: bool,
    /// Send only this app's audio (executable name, e.g. "spotify.exe"); `None` sends everything.
    /// Windows 10 version 2004 and later.
    pub app: Option<String>,
    /// Send everything except `app` instead of only `app`.
    pub exclude_app: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DesktopSettings {
    pub launch_at_login: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub prevent_sleep_while_streaming: bool,
    pub mute_hotkey: Option<String>,
    pub push_to_talk_hotkey: Option<String>,
    /// Output device used as the virtual microphone feed (compatibility mode).
    pub virtual_mic_device: Option<String>,
    /// Start the phone microphone when another app opens the virtual microphone.
    pub auto_start_mic: bool,
    /// Device the phone microphone was last used with; `auto_start_mic` prefers it.
    pub last_mic_peer: Option<String>,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            launch_at_login: true,
            start_minimized: true,
            close_to_tray: true,
            prevent_sleep_while_streaming: false,
            mute_hotkey: None,
            push_to_talk_hotkey: None,
            virtual_mic_device: None,
            auto_start_mic: false,
            last_mic_peer: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct MobileSettings {
    pub stay_available: bool,
    pub remind_after_restart: bool,
}

/// A route the user asked to keep running across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRoute {
    pub peer_id: String,
    pub kind: crate::state::RouteKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    pub device_name: String,
    pub theme: Theme,
    /// BCP-47 tag or "system".
    pub language: String,
    pub visibility: Visibility,
    pub stream: StreamSettings,
    pub output: OutputSettings,
    pub mic: MicSettings,
    pub capture: CaptureSettings,
    pub desktop: DesktopSettings,
    pub mobile: MobileSettings,
    pub auto_connect_trusted: bool,
    pub resume_routes_on_start: bool,
    pub saved_routes: Vec<SavedRoute>,
    pub dismissed_tips: Vec<String>,
    pub audio_cues: bool,
    /// Look for a new SoundPush release in the background (GitHub Releases). Never installs by itself.
    pub check_for_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            device_name: String::new(),
            theme: Theme::System,
            language: "system".into(),
            visibility: Visibility::TrustedOnly,
            stream: StreamSettings::default(),
            output: OutputSettings::default(),
            mic: MicSettings::default(),
            capture: CaptureSettings::default(),
            desktop: DesktopSettings::default(),
            mobile: MobileSettings::default(),
            auto_connect_trusted: true,
            resume_routes_on_start: false,
            saved_routes: Vec::new(),
            dismissed_tips: Vec::new(),
            audio_cues: false,
            check_for_updates: true,
        }
    }
}

impl Settings {
    /// Clamp values into valid ranges (protects against hand-edited files).
    pub fn sanitize(&mut self) {
        self.device_name = self.device_name.trim().chars().take(64).collect();
        self.output.volume = self.output.volume.clamp(0.0, 2.0);
        self.output.balance = self.output.balance.clamp(-1.0, 1.0);
        self.output.av_offset_ms = self.output.av_offset_ms.clamp(-500, 500);
        self.mic.gain_db = self.mic.gain_db.clamp(0.0, 20.0);
        self.stream.opus_bitrate = self.stream.opus_bitrate.clamp(6_000, 510_000);
        self.stream.custom_min_ms = self.stream.custom_min_ms.clamp(5, 500);
        self.stream.custom_max_ms = self.stream.custom_max_ms.clamp(self.stream.custom_min_ms, 1000);
        self.capture.app = self.capture.app.take().map(|a| a.trim().chars().take(260).collect()).filter(|a: &String| !a.is_empty());
        self.dismissed_tips.truncate(256);
        self.saved_routes.truncate(32);
        self.version = SETTINGS_VERSION;
    }
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("settings.json"),
        }
    }

    /// Returns settings and whether a corrupted file was recovered.
    pub fn load(&self) -> (Settings, bool) {
        match fs::read(&self.path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(mut s) => {
                    s.sanitize();
                    (s, false)
                }
                Err(_) => {
                    let _ = fs::rename(&self.path, self.path.with_extension("json.bad"));
                    (Settings::default(), true)
                }
            },
            Err(_) => (Settings::default(), false),
        }
    }

    pub fn save(&self, settings: &Settings) -> Result<(), crate::EngineError> {
        let json = serde_json::to_vec_pretty(settings).map_err(|e| crate::EngineError::Storage(e.to_string()))?;
        write_atomic(&self.path, &json).map_err(|e| crate::EngineError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path());
        let mut s = Settings::default();
        s.device_name = "Desk".into();
        s.theme = Theme::Dark;
        store.save(&s).unwrap();
        assert_eq!(store.load().0, s);

        fs::write(dir.path().join("settings.json"), br#"{"deviceName":"Old","futureField":1}"#).unwrap();
        let (loaded, recovered) = store.load();
        assert!(!recovered);
        assert_eq!(loaded.device_name, "Old");
        assert_eq!(loaded.theme, Theme::System);
        // Files written before the update check existed keep it on.
        assert!(loaded.check_for_updates);
    }

    #[test]
    fn corrupted_file_recovers_defaults() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("settings.json"), b"{not json").unwrap();
        let (s, recovered) = SettingsStore::new(dir.path()).load();
        assert!(recovered);
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn sanitize_clamps() {
        let mut s = Settings::default();
        s.mic.gain_db = 99.0;
        s.output.volume = -1.0;
        s.stream.custom_min_ms = 900;
        s.stream.custom_max_ms = 10;
        s.sanitize();
        assert_eq!(s.mic.gain_db, 20.0);
        assert_eq!(s.output.volume, 0.0);
        assert!(s.stream.custom_max_ms >= s.stream.custom_min_ms);
    }
}
