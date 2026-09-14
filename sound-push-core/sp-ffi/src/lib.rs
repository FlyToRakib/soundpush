//! Mobile bindings.
//!
//! The FFI surface is deliberately small: commands take simple values, and the
//! full [`sp_engine::EngineState`] crosses the boundary as JSON (parsed by
//! kotlinx.serialization / Swift Codable). This keeps bindings stable while the
//! state model evolves.
//!
//! Blocking methods must be called off the UI thread (Kotlin: `Dispatchers.IO`).

#[cfg(target_os = "android")]
mod android;
mod app_audio;

use std::path::PathBuf;
use std::sync::Arc;

use sp_audio_io::{AudioBackend, CaptureSource};
use sp_engine::settings::Settings;
use sp_engine::state::RouteKind;
use sp_engine::{
    DeviceProfile, EngineConfig, EngineError, EngineHandle, KeepAlive, PermissionKind,
    PlatformHooks, Policy,
};

use crate::app_audio::{APP_AUDIO_DEVICE, MobileAudioBackend};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    // Not named `message`: Kotlin exceptions already define that property.
    #[error("{detail}")]
    Engine {
        key: String,
        detail: String,
        fix: Option<String>,
    },
}

impl From<EngineError> for FfiError {
    fn from(e: EngineError) -> Self {
        let view = sp_engine::ErrorView::from(&e);
        Self::Engine {
            key: view.key.to_string(),
            detail: view.message,
            fix: view
                .fix
                .and_then(|f| serde_json::to_value(f).ok())
                .and_then(|v| v.as_str().map(str::to_string)),
        }
    }
}

fn invalid(what: &str) -> FfiError {
    EngineError::InvalidInput(what.into()).into()
}

/// Implemented by the app (Kotlin/Swift).
#[uniffi::export(with_foreign)]
pub trait MobilePlatform: Send + Sync {
    /// Absolute path of the app's private files directory.
    fn data_dir(&self) -> String;
    /// 32 random bytes protected by the OS keystore.
    fn storage_key(&self) -> Vec<u8>;
    fn device_name(&self) -> String;
    fn microphone_permitted(&self) -> bool;
    /// Whether app-audio capture (Android 10+ playback capture) can be offered.
    fn app_audio_supported(&self) -> bool;
    /// Start/update/stop the foreground service with these activity types.
    fn keep_alive(&self, playback: bool, microphone: bool, app_audio: bool);
    /// A prompt needs the user while the app may be in the background.
    fn attention_needed(&self, key: String, peer_name: String);
}

/// Receives state updates as JSON.
#[uniffi::export(with_foreign)]
pub trait StateListener: Send + Sync {
    fn on_state(&self, state_json: String);
}

struct Hooks {
    platform: Arc<dyn MobilePlatform>,
    backend: Arc<MobileAudioBackend>,
}

impl PlatformHooks for Hooks {
    fn data_dir(&self) -> PathBuf {
        PathBuf::from(self.platform.data_dir())
    }

    fn storage_key(&self) -> [u8; 32] {
        let bytes = self.platform.storage_key();
        let mut key = [0u8; 32];
        for (dst, src) in key.iter_mut().zip(bytes.iter()) {
            *dst = *src;
        }
        key
    }

    fn platform(&self) -> &'static str {
        if cfg!(target_os = "ios") {
            "ios"
        } else {
            "android"
        }
    }

    fn default_device_name(&self) -> String {
        self.platform.device_name()
    }

    fn audio_backend(&self) -> Arc<dyn AudioBackend> {
        self.backend.clone()
    }

    fn app_audio_source(&self) -> Option<CaptureSource> {
        self.platform
            .app_audio_supported()
            .then(|| CaptureSource::Input(APP_AUDIO_DEVICE.to_string()))
    }

    fn virtual_mic_target(&self, _configured: Option<&str>) -> Option<sp_audio_io::RenderTarget> {
        None
    }

    fn microphone_permitted(&self) -> bool {
        self.platform.microphone_permitted()
    }

    fn keep_alive(&self, reason: KeepAlive) {
        self.platform
            .keep_alive(reason.playback, reason.microphone, reason.app_audio_capture);
    }

    fn attention_needed(&self, title_key: &str, peer_name: &str) {
        self.platform
            .attention_needed(title_key.to_string(), peer_name.to_string());
    }
}

#[derive(uniffi::Object)]
pub struct SoundPushEngine {
    handle: EngineHandle,
    backend: Arc<MobileAudioBackend>,
}

fn parse_kind(kind: &str) -> Result<RouteKind, FfiError> {
    serde_json::from_value(serde_json::Value::String(kind.to_string()))
        .map_err(|_| invalid("route kind"))
}

#[uniffi::export]
impl SoundPushEngine {
    #[uniffi::constructor]
    pub fn new(
        platform: Arc<dyn MobilePlatform>,
        app_version: String,
    ) -> Result<Arc<Self>, FfiError> {
        init_logging();
        // Rust panics leave a local, redacted report (never uploaded); the engine mentions it once.
        sp_engine::crash::install(&PathBuf::from(platform.data_dir()), &app_version);
        let backend = Arc::new(MobileAudioBackend::new());
        let hooks = Arc::new(Hooks {
            platform,
            backend: backend.clone(),
        });
        let handle = EngineHandle::start(
            hooks,
            EngineConfig {
                app_version,
                // Phones dial a computer's USB forward; they never listen on TCP.
                tcp_listener: false,
                ..EngineConfig::default()
            },
        )?;
        Ok(Arc::new(Self { handle, backend }))
    }

    pub fn state_json(&self) -> String {
        serde_json::to_string(&*self.handle.state()).unwrap_or_default()
    }

    /// Deliver every state change to `listener` on a background thread.
    pub fn set_listener(&self, listener: Arc<dyn StateListener>) {
        let mut rx = self.handle.subscribe();
        std::thread::Builder::new()
            .name("sp-state".into())
            .spawn(move || {
                listener.on_state(
                    serde_json::to_string(&*rx.borrow_and_update().clone()).unwrap_or_default(),
                );
                while pollster::block_on(rx.changed()).is_ok() {
                    let state = rx.borrow_and_update().clone();
                    listener.on_state(serde_json::to_string(&*state).unwrap_or_default());
                }
            })
            .ok();
    }

    // ------------------------------------------------------------ pairing

    pub fn start_pairing(&self) -> Result<String, FfiError> {
        Ok(pollster::block_on(self.handle.start_pairing())?)
    }

    pub fn stop_pairing(&self) -> Result<(), FfiError> {
        Ok(self.handle.stop_pairing()?)
    }

    pub fn pair_with_qr(&self, uri: String) -> Result<(), FfiError> {
        Ok(pollster::block_on(self.handle.pair_with_qr(uri))?)
    }

    pub fn pair_with_device(&self, device_id: String) -> Result<(), FfiError> {
        Ok(pollster::block_on(self.handle.pair_with_device(device_id))?)
    }

    pub fn pair_with_address(&self, address: String) -> Result<(), FfiError> {
        Ok(pollster::block_on(self.handle.pair_with_address(address))?)
    }

    pub fn confirm_pairing(&self, device_id: String, accept: bool) -> Result<(), FfiError> {
        Ok(self.handle.confirm_pairing(device_id, accept)?)
    }

    // ------------------------------------------------------------ devices

    pub fn connect(&self, device_id: String) -> Result<(), FfiError> {
        Ok(self.handle.connect(device_id)?)
    }

    pub fn disconnect(&self, device_id: String) -> Result<(), FfiError> {
        Ok(self.handle.disconnect(device_id)?)
    }

    pub fn forget_device(&self, device_id: String) -> Result<(), FfiError> {
        Ok(self.handle.forget_device(device_id)?)
    }

    pub fn set_device_blocked(&self, device_id: String, blocked: bool) -> Result<(), FfiError> {
        Ok(self.handle.set_device_blocked(device_id, blocked)?)
    }

    pub fn rename_device(&self, device_id: String, alias: Option<String>) -> Result<(), FfiError> {
        Ok(self.handle.rename_device(device_id, alias)?)
    }

    pub fn set_auto_connect(&self, device_id: String, enabled: bool) -> Result<(), FfiError> {
        Ok(self.handle.set_auto_connect(device_id, enabled)?)
    }

    pub fn set_permission(
        &self,
        device_id: String,
        kind: String,
        policy: String,
    ) -> Result<(), FfiError> {
        let kind = match kind.as_str() {
            "receiveMyAudio" => PermissionKind::ReceiveMyAudio,
            "useMyMicrophone" => PermissionKind::UseMyMicrophone,
            "sendAudioToMe" => PermissionKind::SendAudioToMe,
            "controlMe" => PermissionKind::ControlMe,
            _ => return Err(invalid("permission")),
        };
        let policy = match policy.as_str() {
            "allow" => Policy::Allow,
            "ask" => Policy::Ask,
            "deny" => Policy::Deny,
            _ => return Err(invalid("policy")),
        };
        Ok(self.handle.set_permission(device_id, kind, policy)?)
    }

    /// `profile_json`: a `DeviceProfile` (absent fields follow the global setting); `None` clears it.
    pub fn set_device_profile(
        &self,
        device_id: String,
        profile_json: Option<String>,
    ) -> Result<(), FfiError> {
        let profile = match profile_json {
            Some(json) => {
                Some(serde_json::from_str::<DeviceProfile>(&json).map_err(|_| invalid("profile"))?)
            }
            None => None,
        };
        Ok(self.handle.set_device_profile(device_id, profile)?)
    }

    /// Blocks for the whole test (about 10 s); returns the `NetworkReport` as JSON.
    /// Progress and the result are also in the state (`networkTests`).
    pub fn run_network_test(&self, device_id: String) -> Result<String, FfiError> {
        let report = pollster::block_on(self.handle.run_network_test(device_id))?;
        Ok(serde_json::to_string(&report).unwrap_or_default())
    }

    pub fn cancel_network_test(&self, device_id: String) -> Result<(), FfiError> {
        Ok(self.handle.cancel_network_test(device_id)?)
    }

    // ------------------------------------------------------------ routes

    pub fn start_route(&self, device_id: String, kind: String) -> Result<String, FfiError> {
        let kind = parse_kind(&kind)?;
        Ok(pollster::block_on(
            self.handle.start_route(device_id, kind),
        )?)
    }

    pub fn stop_route(&self, route_id: String) -> Result<(), FfiError> {
        Ok(self.handle.stop_route(route_id)?)
    }

    pub fn set_route_volume(&self, route_id: String, volume: f32) -> Result<(), FfiError> {
        Ok(self.handle.set_route_volume(route_id, volume)?)
    }

    pub fn set_route_muted(&self, route_id: String, muted: bool) -> Result<(), FfiError> {
        Ok(self.handle.set_route_muted(route_id, muted)?)
    }

    pub fn set_route_keep_running(&self, route_id: String, keep: bool) -> Result<(), FfiError> {
        Ok(self.handle.set_route_keep_running(route_id, keep)?)
    }

    pub fn set_peer_speakers_muted(&self, device_id: String, muted: bool) -> Result<(), FfiError> {
        Ok(self.handle.set_peer_speakers_muted(device_id, muted)?)
    }

    pub fn respond_route_request(
        &self,
        request_id: u64,
        accept: bool,
        remember: bool,
    ) -> Result<(), FfiError> {
        Ok(self
            .handle
            .respond_route_request(request_id, accept, remember)?)
    }

    // ------------------------------------------------------------ local audio

    pub fn set_mic_muted(&self, muted: bool) -> Result<(), FfiError> {
        Ok(self.handle.set_mic_muted(muted)?)
    }

    pub fn set_mic_monitor(&self, enabled: bool) -> Result<(), FfiError> {
        Ok(self.handle.set_mic_monitor(enabled)?)
    }

    /// Feed 48 kHz interleaved stereo 16-bit little-endian PCM from Android playback capture.
    /// Bytes avoid boxing every sample on the Kotlin side.
    pub fn push_app_audio_pcm16(&self, pcm: Vec<u8>) {
        self.backend.push_app_audio_pcm16(&pcm);
    }

    /// True while an app-audio route is capturing (the app should run AudioRecord).
    pub fn app_audio_active(&self) -> bool {
        self.backend.app_audio_active()
    }

    /// Feed 48 kHz mono 16-bit little-endian PCM from the microphone recorder.
    pub fn push_mic_pcm16(&self, pcm: Vec<u8>) {
        self.backend.push_mic_pcm16(&pcm);
    }

    /// True while a microphone route or mic monitor needs input (the app should run AudioRecord).
    pub fn mic_capture_active(&self) -> bool {
        self.backend.mic_capture_active()
    }

    /// Play through the platform (Android AudioTrack) instead of the low-latency path, so the
    /// device's effects and equalizers apply. Running streams switch live.
    pub fn set_platform_output(&self, enabled: bool) {
        self.backend.set_platform_output(enabled);
    }

    /// True while any stream plays through the platform path (the app should run AudioTrack),
    /// either because the user chose it or because the low-latency path failed to open.
    pub fn platform_output_active(&self) -> bool {
        self.backend.platform_output_active()
    }

    /// Mixed 48 kHz interleaved stereo s16le for the platform player: `frames` frames, silence
    /// when nothing plays there. Call from the player thread, never the UI thread.
    pub fn pull_playback_pcm16(&self, frames: u32) -> Vec<u8> {
        self.backend.pull_playback_pcm16(frames as usize)
    }

    // ------------------------------------------------------------ settings & lifecycle

    pub fn update_settings(&self, settings_json: String) -> Result<String, FfiError> {
        let settings: Settings =
            serde_json::from_str(&settings_json).map_err(|_| invalid("settings"))?;
        let saved = pollster::block_on(self.handle.update_settings(settings))?;
        Ok(serde_json::to_string(&saved).unwrap_or_default())
    }

    pub fn dismiss_notice(&self, id: u64) -> Result<(), FfiError> {
        Ok(self.handle.dismiss_notice(id)?)
    }

    /// The local security log as a JSON array of `AuditEntry`, newest first.
    pub fn audit_log_json(&self) -> Result<String, FfiError> {
        let entries = pollster::block_on(self.handle.audit_log())?;
        Ok(serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into()))
    }

    /// Delete the security log (a "log cleared" entry remains).
    pub fn clear_audit_log(&self) -> Result<(), FfiError> {
        Ok(pollster::block_on(self.handle.clear_audit_log())?)
    }

    pub fn network_changed(&self) -> Result<(), FfiError> {
        Ok(self.handle.network_changed()?)
    }

    pub fn set_foreground(&self, foreground: bool) -> Result<(), FfiError> {
        Ok(self.handle.set_foreground(foreground)?)
    }
}

#[cfg(target_os = "android")]
fn init_logging() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("SoundPush")
            .with_max_level(log::LevelFilter::Info),
    );
}

#[cfg(not(target_os = "android"))]
fn init_logging() {}
