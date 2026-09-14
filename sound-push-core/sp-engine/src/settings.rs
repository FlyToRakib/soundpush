//! Persistent settings (one schema for every platform).
//!
//! Stored as JSON with atomic writes. Unknown fields are ignored and missing
//! fields take defaults, so older and newer versions can read each other's files.
//! A corrupted file is moved aside and defaults are used.
//!
//! `version` records the schema a file was written with. Files from older versions go through
//! [`migrate`] (one step per version, on the raw JSON) before they are parsed, and are written
//! back in the current schema. Add a step whenever a change needs more than a serde default.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sp_media::profile::{LatencyProfile, Quality};
use sp_security::secretbox::write_atomic;

/// 2: `savedRoutes[].keep` is always written; microphone high-pass filter.
pub const SETTINGS_VERSION: u32 = 2;

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
    /// 80 Hz high-pass before noise suppression (rumble, handling and wind noise; plan §15.7).
    pub high_pass: bool,
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
            high_pass: true,
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

/// A route restored when its device connects after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRoute {
    pub peer_id: String,
    pub kind: crate::state::RouteKind,
    /// True when the user chose "Keep running after restart" for this route. False when it was
    /// saved because "Resume streams after restart" is on; such entries go away when the route stops.
    #[serde(default = "default_true")]
    pub keep: bool,
}

fn default_true() -> bool {
    true
}

impl SavedRoute {
    pub fn matches(&self, peer_id: &str, kind: crate::state::RouteKind) -> bool {
        self.peer_id == peer_id && self.kind == kind
    }
}

/// Per-device overrides of the stream settings. `None` follows the global setting.
///
/// Latency applies to audio this device receives from the peer; quality and redundancy apply to
/// routes this device starts (the requester proposes the codec, the receiver picks its buffer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct DeviceProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<LatencyMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_min_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_max_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<QualityMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opus_bitrate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redundancy: Option<bool>,
}

/// Paired devices can each have a profile; more than this is a hand-edited file.
pub const MAX_DEVICE_PROFILES: usize = 64;

impl DeviceProfile {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The global stream settings with this profile's overrides applied.
    pub fn apply(&self, base: &StreamSettings) -> StreamSettings {
        let mut s = base.clone();
        if let Some(v) = self.latency {
            s.latency = v;
        }
        if let Some(v) = self.custom_min_ms {
            s.custom_min_ms = v;
        }
        if let Some(v) = self.custom_max_ms {
            s.custom_max_ms = v;
        }
        if let Some(v) = self.quality {
            s.quality = v;
        }
        if let Some(v) = self.opus_bitrate {
            s.opus_bitrate = v;
        }
        if let Some(v) = self.redundancy {
            s.redundancy = v;
        }
        s.custom_min_ms = s.custom_min_ms.clamp(5, 500);
        s.custom_max_ms = s.custom_max_ms.clamp(s.custom_min_ms, 1000);
        s.opus_bitrate = s.opus_bitrate.clamp(6_000, 510_000);
        s
    }

    fn sanitize(&mut self) {
        self.opus_bitrate = self.opus_bitrate.map(|b| b.clamp(6_000, 510_000));
        self.custom_min_ms = self.custom_min_ms.map(|v| v.clamp(5, 500));
        self.custom_max_ms = self.custom_max_ms.map(|v| v.clamp(5, 1000));
    }
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
    /// Per-device stream overrides, keyed by device id (hex).
    pub device_profiles: std::collections::BTreeMap<String, DeviceProfile>,
    /// Look for a new SoundPush release in the background (GitHub Releases). Never installs by itself.
    pub check_for_updates: bool,
    /// Which releases the desktop updater offers (docs/soundpush-final.md §32).
    pub update_channel: UpdateChannel,
    /// Write debug-level logs for troubleshooting (plan §28.1). Switches itself off after
    /// [`DEBUG_LOGGING_SECS`].
    pub debug_logging: bool,
    /// When debug logging switches itself off (unix seconds; 0 while off). Set by the engine.
    pub debug_logging_until_unix: u64,
}

/// Desktop update channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChannel {
    /// Pre-releases (`x.y.z-beta.n`) as soon as they are published, and every stable release.
    Beta,
    /// Published releases, reaching installs in stages. Also used for unknown values from newer versions
    /// (`#[serde(other)]` must stay on the last variant).
    #[default]
    #[serde(other)]
    Stable,
}

/// Debug logging switches itself off after this long.
pub const DEBUG_LOGGING_SECS: u64 = 24 * 3600;

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
            device_profiles: std::collections::BTreeMap::new(),
            check_for_updates: true,
            update_channel: UpdateChannel::Stable,
            debug_logging: false,
            debug_logging_until_unix: 0,
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
        self.stream.custom_max_ms = self
            .stream
            .custom_max_ms
            .clamp(self.stream.custom_min_ms, 1000);
        self.capture.app = self
            .capture
            .app
            .take()
            .map(|a| a.trim().chars().take(260).collect())
            .filter(|a: &String| !a.is_empty());
        self.dismissed_tips.truncate(256);
        self.saved_routes.truncate(32);
        self.device_profiles.retain(|id, p| {
            id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) && !p.is_empty()
        });
        for p in self.device_profiles.values_mut() {
            p.sanitize();
        }
        while self.device_profiles.len() > MAX_DEVICE_PROFILES {
            self.device_profiles.pop_last();
        }
        self.version = SETTINGS_VERSION;
    }

    /// Give debug logging its end time: [`DEBUG_LOGGING_SECS`] after it was switched on (never
    /// later, whatever a client sends). Off clears it. `was_on`: the setting before this change.
    pub fn schedule_debug_logging(&mut self, was_on: bool, now_unix: u64) {
        let latest = now_unix.saturating_add(DEBUG_LOGGING_SECS);
        self.debug_logging_until_unix = match (self.debug_logging, was_on) {
            (false, _) => 0,
            (true, false) => latest,
            (true, true) if self.debug_logging_until_unix == 0 => latest,
            (true, true) => self.debug_logging_until_unix.min(latest),
        };
    }

    /// Switch debug logging off once its time is up. True when it changed.
    pub fn expire_debug_logging(&mut self, now_unix: u64) -> bool {
        if self.debug_logging && now_unix >= self.debug_logging_until_unix {
            self.debug_logging = false;
            self.debug_logging_until_unix = 0;
            return true;
        }
        false
    }

    /// Stream settings for routes with `peer_id` (hex): the global settings plus the device's profile.
    pub fn stream_for(&self, peer_id: &str) -> StreamSettings {
        match self.device_profiles.get(peer_id) {
            Some(profile) => profile.apply(&self.stream),
            None => self.stream.clone(),
        }
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
        let Ok(bytes) = fs::read(&self.path) else {
            return (Settings::default(), false);
        };
        let doc = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .filter(Value::is_object);
        let Some(mut doc) = doc else {
            return self.recover();
        };
        // Files written before the version hook existed all said 1.
        let from = doc
            .get("version")
            .and_then(Value::as_u64)
            .map_or(1, |v| u32::try_from(v).unwrap_or(u32::MAX));
        if from > SETTINGS_VERSION {
            tracing::warn!(
                from,
                current = SETTINGS_VERSION,
                "settings from a newer version; unknown fields are ignored"
            );
        }
        let migrated = migrate(&mut doc, from);
        match serde_json::from_value::<Settings>(doc) {
            Ok(mut s) => {
                s.sanitize();
                if migrated {
                    if let Err(e) = self.save(&s) {
                        tracing::warn!(error = %e, "could not write migrated settings");
                    }
                }
                (s, false)
            }
            Err(_) => self.recover(),
        }
    }

    fn recover(&self) -> (Settings, bool) {
        let _ = fs::rename(&self.path, self.path.with_extension("json.bad"));
        (Settings::default(), true)
    }

    pub fn save(&self, settings: &Settings) -> Result<(), crate::EngineError> {
        let json = serde_json::to_vec_pretty(settings)
            .map_err(|e| crate::EngineError::Storage(e.to_string()))?;
        write_atomic(&self.path, &json).map_err(|e| crate::EngineError::Storage(e.to_string()))
    }
}

/// Bring a settings document written with schema `from` up to [`SETTINGS_VERSION`].
/// Returns true when anything changed (the caller then writes the file back).
/// Documents from newer versions are left alone.
fn migrate(doc: &mut Value, from: u32) -> bool {
    let mut version = from.max(1);
    if version >= SETTINGS_VERSION {
        return false;
    }
    while version < SETTINGS_VERSION {
        if version == 1 {
            // 1 → 2: a saved route without `keep` was chosen explicitly ("Keep running").
            if let Some(routes) = doc.get_mut("savedRoutes").and_then(Value::as_array_mut) {
                for route in routes.iter_mut().filter_map(Value::as_object_mut) {
                    route.entry("keep").or_insert(Value::Bool(true));
                }
            }
        }
        version += 1;
    }
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("version".into(), Value::from(version));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path());
        let s = Settings {
            device_name: "Desk".into(),
            theme: Theme::Dark,
            ..Settings::default()
        };
        store.save(&s).unwrap();
        assert_eq!(store.load().0, s);

        fs::write(
            dir.path().join("settings.json"),
            br#"{"deviceName":"Old","futureField":1}"#,
        )
        .unwrap();
        let (loaded, recovered) = store.load();
        assert!(!recovered);
        assert_eq!(loaded.device_name, "Old");
        assert_eq!(loaded.theme, Theme::System);
        // Files written before the update check existed keep it on.
        assert!(loaded.check_for_updates);
    }

    #[test]
    fn older_files_are_migrated_and_written_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            br#"{"version":1,"deviceName":"Old","savedRoutes":[{"peerId":"ab","kind":"sendSystemAudio"},{"peerId":"cd","kind":"sendSystemAudio","keep":false}]}"#,
        )
        .unwrap();
        let (s, recovered) = SettingsStore::new(dir.path()).load();
        assert!(!recovered);
        assert_eq!(s.version, SETTINGS_VERSION);
        assert!(s.saved_routes[0].keep);
        assert!(!s.saved_routes[1].keep, "explicit values are kept");
        assert!(s.mic.high_pass, "new fields take their defaults");

        let written: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["version"], SETTINGS_VERSION);
        assert_eq!(written["savedRoutes"][0]["keep"], true);

        // Current and newer files are not rewritten by a load.
        let mut newer: Value = written.clone();
        newer["version"] = Value::from(SETTINGS_VERSION + 1);
        assert!(!migrate(&mut newer, SETTINGS_VERSION + 1));
        let mut current = written;
        assert!(!migrate(&mut current, SETTINGS_VERSION));
    }

    #[test]
    fn debug_logging_switches_itself_off_after_a_day() {
        let now = 1_800_000_000;
        let mut s = Settings {
            debug_logging: true,
            ..Settings::default()
        };
        s.schedule_debug_logging(false, now);
        assert_eq!(s.debug_logging_until_unix, now + DEBUG_LOGGING_SECS);
        // Later changes keep the end; a client cannot push it further out.
        s.debug_logging_until_unix = u64::MAX;
        s.schedule_debug_logging(true, now + 60);
        assert_eq!(s.debug_logging_until_unix, now + 60 + DEBUG_LOGGING_SECS);
        s.schedule_debug_logging(true, now + 120);
        assert_eq!(s.debug_logging_until_unix, now + 60 + DEBUG_LOGGING_SECS);

        assert!(!s.expire_debug_logging(now + 3600));
        assert!(s.expire_debug_logging(now + 60 + DEBUG_LOGGING_SECS));
        assert!(!s.debug_logging);
        assert_eq!(s.debug_logging_until_unix, 0);
        s.schedule_debug_logging(true, now);
        assert_eq!(s.debug_logging_until_unix, 0, "off stays off");
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
    fn update_channel_defaults_and_unknown_values() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path());
        assert_eq!(Settings::default().update_channel, UpdateChannel::Stable);

        fs::write(
            dir.path().join("settings.json"),
            br#"{"updateChannel":"beta"}"#,
        )
        .unwrap();
        assert_eq!(store.load().0.update_channel, UpdateChannel::Beta);

        // A channel written by a newer version must not reset every other setting.
        fs::write(
            dir.path().join("settings.json"),
            br#"{"deviceName":"Desk","updateChannel":"nightly"}"#,
        )
        .unwrap();
        let (loaded, recovered) = store.load();
        assert!(!recovered);
        assert_eq!(loaded.device_name, "Desk");
        assert_eq!(loaded.update_channel, UpdateChannel::Stable);
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

    #[test]
    fn device_profiles_override_and_sanitize() {
        let peer = "0123456789abcdef0123456789abcdef".to_string();
        let mut s = Settings::default();
        s.device_profiles.insert(
            peer.clone(),
            DeviceProfile {
                latency: Some(LatencyMode::Stable),
                opus_bitrate: Some(1),
                ..DeviceProfile::default()
            },
        );
        s.device_profiles.insert(
            "not-a-device".into(),
            DeviceProfile {
                redundancy: Some(true),
                ..DeviceProfile::default()
            },
        );
        s.device_profiles.insert(
            "fedcba9876543210fedcba9876543210".into(),
            DeviceProfile::default(),
        );
        s.sanitize();
        assert_eq!(
            s.device_profiles.len(),
            1,
            "invalid ids and empty profiles are dropped"
        );
        let stream = s.stream_for(&peer);
        assert_eq!(stream.latency, LatencyMode::Stable);
        assert_eq!(stream.opus_bitrate, 6_000);
        assert_eq!(
            stream.quality, s.stream.quality,
            "unset fields follow the global setting"
        );
        assert_eq!(s.stream_for("other"), s.stream);

        // Round trip, and files from before profiles/keep existed still load.
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"deviceProfiles\"") && !json.contains("\"customMinMs\":null"));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        let old: Settings =
            serde_json::from_str(r#"{"savedRoutes":[{"peerId":"ab","kind":"sendSystemAudio"}]}"#)
                .unwrap();
        assert!(
            old.saved_routes[0].keep,
            "routes saved by older versions were explicit"
        );
    }
}
