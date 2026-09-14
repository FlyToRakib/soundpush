//! Tauri commands: thin wrappers over the engine API.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sp_engine::settings::Settings;
use sp_engine::state::RouteKind;
use sp_engine::{EngineError, EngineState, ErrorView, PermissionKind, Policy, Severity};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;

type CmdResult<T> = Result<T, CommandError>;

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct CommandError(ErrorView);

impl From<EngineError> for CommandError {
    fn from(e: EngineError) -> Self {
        Self(ErrorView::from(&e))
    }
}

impl CommandError {
    /// An error the engine does not know about (desktop-only features), by i18n key.
    fn keyed(key: &'static str, message: impl Into<String>) -> Self {
        Self(ErrorView {
            key,
            message: message.into(),
            severity: Severity::Warning,
            retryable: false,
            fix: None,
        })
    }
}

fn invalid(what: &str) -> CommandError {
    EngineError::InvalidInput(what.to_string()).into()
}

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> Option<Arc<EngineState>> {
    state.engine.get().map(|e| e.state())
}

#[tauri::command]
pub async fn start_pairing(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(state.engine()?.start_pairing().await?)
}

#[tauri::command]
pub fn stop_pairing(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(state.engine()?.stop_pairing()?)
}

#[tauri::command]
pub async fn pair_with_address(state: State<'_, AppState>, address: String) -> CmdResult<()> {
    Ok(state.engine()?.pair_with_address(address).await?)
}

#[tauri::command]
pub async fn pair_with_device(state: State<'_, AppState>, device_id: String) -> CmdResult<()> {
    Ok(state.engine()?.pair_with_device(device_id).await?)
}

#[tauri::command]
pub async fn pair_with_qr(state: State<'_, AppState>, uri: String) -> CmdResult<()> {
    Ok(state.engine()?.pair_with_qr(uri).await?)
}

#[tauri::command]
pub fn confirm_pairing(state: State<'_, AppState>, device_id: String, accept: bool) -> CmdResult<()> {
    Ok(state.engine()?.confirm_pairing(device_id, accept)?)
}

#[tauri::command]
pub fn connect_device(state: State<'_, AppState>, device_id: String) -> CmdResult<()> {
    Ok(state.engine()?.connect(device_id)?)
}

#[tauri::command]
pub fn disconnect_device(state: State<'_, AppState>, device_id: String) -> CmdResult<()> {
    Ok(state.engine()?.disconnect(device_id)?)
}

#[tauri::command]
pub fn forget_device(state: State<'_, AppState>, device_id: String) -> CmdResult<()> {
    Ok(state.engine()?.forget_device(device_id)?)
}

#[tauri::command]
pub fn set_device_blocked(state: State<'_, AppState>, device_id: String, blocked: bool) -> CmdResult<()> {
    Ok(state.engine()?.set_device_blocked(device_id, blocked)?)
}

#[tauri::command]
pub fn rename_device(state: State<'_, AppState>, device_id: String, alias: Option<String>) -> CmdResult<()> {
    Ok(state.engine()?.rename_device(device_id, alias)?)
}

#[tauri::command]
pub fn set_auto_connect(state: State<'_, AppState>, device_id: String, enabled: bool) -> CmdResult<()> {
    Ok(state.engine()?.set_auto_connect(device_id, enabled)?)
}

#[tauri::command]
pub fn set_permission(state: State<'_, AppState>, device_id: String, kind: String, policy: String) -> CmdResult<()> {
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
    Ok(state.engine()?.set_permission(device_id, kind, policy)?)
}

#[tauri::command]
pub async fn start_route(state: State<'_, AppState>, device_id: String, kind: RouteKind) -> CmdResult<String> {
    Ok(state.engine()?.start_route(device_id, kind).await?)
}

#[tauri::command]
pub fn stop_route(state: State<'_, AppState>, route_id: String) -> CmdResult<()> {
    Ok(state.engine()?.stop_route(route_id)?)
}

#[tauri::command]
pub fn set_route_volume(state: State<'_, AppState>, route_id: String, volume: f32) -> CmdResult<()> {
    Ok(state.engine()?.set_route_volume(route_id, volume)?)
}

#[tauri::command]
pub fn set_route_muted(state: State<'_, AppState>, route_id: String, muted: bool) -> CmdResult<()> {
    Ok(state.engine()?.set_route_muted(route_id, muted)?)
}

#[tauri::command]
pub fn set_route_keep_running(state: State<'_, AppState>, route_id: String, keep: bool) -> CmdResult<()> {
    Ok(state.engine()?.set_route_keep_running(route_id, keep)?)
}

#[tauri::command]
pub fn set_peer_speakers_muted(state: State<'_, AppState>, device_id: String, muted: bool) -> CmdResult<()> {
    Ok(state.engine()?.set_peer_speakers_muted(device_id, muted)?)
}

#[tauri::command]
pub fn respond_route_request(state: State<'_, AppState>, request_id: u64, accept: bool, remember: bool) -> CmdResult<()> {
    Ok(state.engine()?.respond_route_request(request_id, accept, remember)?)
}

#[tauri::command]
pub fn set_mic_muted(state: State<'_, AppState>, muted: bool) -> CmdResult<()> {
    Ok(state.engine()?.set_mic_muted(muted)?)
}

#[tauri::command]
pub fn set_mic_monitor(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    Ok(state.engine()?.set_mic_monitor(enabled)?)
}

#[tauri::command]
pub fn refresh_audio_devices(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(state.engine()?.refresh_audio_devices()?)
}

#[tauri::command]
pub async fn update_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    Ok(state.engine()?.update_settings(settings).await?)
}

#[tauri::command]
pub fn dismiss_notice(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    Ok(state.engine()?.dismiss_notice(id)?)
}

/// Why the engine could not start, if it failed; the UI shows it instead of "Starting…".
#[tauri::command]
pub fn get_start_error(state: State<'_, AppState>) -> Option<String> {
    state.start_error.lock().ok().and_then(|e| e.clone())
}

/// Write a diagnostics report the user can inspect and share. Nothing is uploaded.
#[tauri::command]
pub async fn export_diagnostics(state: State<'_, AppState>) -> CmdResult<String> {
    let snapshot = state.engine()?.state();
    let data_dir = state.data_dir.clone();
    let log_dir = state.log_dir.clone();
    // Reading the log can take a while: keep it off the UI thread.
    tauri::async_runtime::spawn_blocking(move || write_diagnostics(&snapshot, &data_dir, &log_dir))
        .await
        .map_err(|e| EngineError::Internal(e.to_string()))?
}

fn write_diagnostics(snapshot: &EngineState, data_dir: &std::path::Path, log_dir: &std::path::Path) -> CmdResult<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let dir = data_dir.join("diagnostics");
    std::fs::create_dir_all(&dir).map_err(|e| EngineError::Storage(e.to_string()))?;
    let path = dir.join(format!("soundpush-diagnostics-{now}.txt"));

    let mut report = String::new();
    report.push_str(&format!(
        "SoundPush {} diagnostics\nOS: {} {}\nGenerated: {now}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    // Redact remote addresses; keep only the local device code and peer prefixes.
    let mut redacted = snapshot.clone();
    for peer in &mut redacted.peers {
        peer.addresses = peer.addresses.iter().map(|_| "<redacted>".to_string()).collect();
        peer.device_id.truncate(8);
    }
    report.push_str("== State ==\n");
    report.push_str(&serde_json::to_string_pretty(&redacted).unwrap_or_default());
    report.push_str("\n\n== Recent log ==\n");
    if let Some(latest) = latest_log(log_dir) {
        if let Ok(file) = std::fs::File::open(latest) {
            let lines: Vec<String> = BufReader::new(file).lines().map_while(Result::ok).collect();
            for line in lines.iter().skip(lines.len().saturating_sub(2000)) {
                report.push_str(line);
                report.push('\n');
            }
        }
    }
    std::fs::write(&path, report).map_err(|e| EngineError::Storage(e.to_string()))?;
    Ok(path.to_string_lossy().to_string())
}

fn latest_log(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("soundpush"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

#[tauri::command]
pub fn open_logs_folder(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    app.opener()
        .open_path(state.log_dir.to_string_lossy(), None::<&str>)
        .map_err(|e| EngineError::Internal(e.to_string()).into())
}

#[tauri::command]
pub fn virtual_mic_status() -> crate::virtual_mic::Status {
    crate::virtual_mic::status()
}

/// Install SoundPush Microphone. Waits while the user answers the OS administrator prompt.
#[tauri::command]
pub async fn install_virtual_mic(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    change_virtual_mic(move || crate::virtual_mic::install(&app)).await?;
    state.engine()?.refresh_audio_devices()?;
    Ok(())
}

#[tauri::command]
pub async fn uninstall_virtual_mic(state: State<'_, AppState>) -> CmdResult<()> {
    change_virtual_mic(crate::virtual_mic::uninstall).await?;
    state.engine()?.refresh_audio_devices()?;
    Ok(())
}

/// Restart the computer to finish installing VB-CABLE (Windows). The UI confirms first.
#[tauri::command]
pub fn restart_computer() -> CmdResult<()> {
    crate::virtual_mic::restart_computer().map_err(|e| EngineError::Internal(e).into())
}

async fn change_virtual_mic(f: impl FnOnce() -> Result<(), String> + Send + 'static) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = f();
        // The audio server needs a moment to restart and publish its devices.
        std::thread::sleep(std::time::Duration::from_secs(2));
        result
    })
    .await
    .map_err(|e| EngineError::Internal(e.to_string()))?
    .map_err(|e| EngineError::Internal(e).into())
}

#[tauri::command]
pub fn hotkey_status(app: AppHandle) -> crate::hotkeys::HotkeyStatus {
    crate::hotkeys::status(&app)
}

/// Set or clear a global shortcut. It is registered first, so a combination another app owns
/// is reported (and not saved); then the settings are saved.
#[tauri::command]
pub async fn set_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: crate::hotkeys::Kind,
    accelerator: Option<String>,
) -> CmdResult<()> {
    let engine = state.engine()?.clone();
    let accelerator = accelerator.map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    {
        // Registration waits on the main thread, so it must not run on it (or on the async runtime).
        let accelerator = accelerator.clone();
        tauri::async_runtime::spawn_blocking(move || crate::hotkeys::set(&app, kind, accelerator.as_deref()))
            .await
            .map_err(|e| EngineError::Internal(e.to_string()))?
            .map_err(|e| CommandError::keyed(e.key(), format!("{e:?}")))?;
    }

    let mut settings = engine.state().settings.clone();
    match kind {
        crate::hotkeys::Kind::Mute => settings.desktop.mute_hotkey = accelerator,
        crate::hotkeys::Kind::PushToTalk => settings.desktop.push_to_talk_hotkey = accelerator,
    }
    engine.update_settings(settings).await?;
    Ok(())
}

/// Firewall and network profile (Windows). Checked fresh; takes a moment on large rule sets.
#[tauri::command]
pub async fn network_status(state: State<'_, AppState>) -> CmdResult<crate::network::NetworkStatus> {
    let status = tauri::async_runtime::spawn_blocking(crate::network::status)
        .await
        .map_err(|e| EngineError::Internal(e.to_string()))?;
    state.hooks.set_network_status(status.clone());
    Ok(status)
}

/// Add SoundPush's firewall rule through a UAC prompt, then check again.
#[tauri::command]
pub async fn fix_firewall(state: State<'_, AppState>, include_public: bool) -> CmdResult<crate::network::NetworkStatus> {
    tauri::async_runtime::spawn_blocking(move || crate::network::fix_firewall(include_public))
        .await
        .map_err(|e| EngineError::Internal(e.to_string()))?
        .map_err(EngineError::Internal)?;
    network_status(state).await
}

#[tauri::command]
pub async fn system_status() -> CmdResult<crate::system::SystemStatus> {
    tauri::async_runtime::spawn_blocking(crate::system::status)
        .await
        .map_err(|e| EngineError::Internal(e.to_string()).into())
}

/// Ask the OS for microphone access (macOS shows its prompt only while undecided).
#[tauri::command]
pub async fn request_microphone(state: State<'_, AppState>) -> CmdResult<()> {
    let backend = state.hooks.backend();
    tauri::async_runtime::spawn_blocking(move || {
        use sp_audio_io::AudioBackend;
        // Opening the microphone is what makes macOS ask; nothing is recorded.
        if let Ok(stream) = backend.open_capture(&sp_audio_io::CaptureSource::DefaultInput, 1, Box::new(|_| {}), Box::new(|_| {})) {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(stream);
        }
    })
    .await
    .map_err(|e| EngineError::Internal(e.to_string()).into())
}

/// Open a System Settings page ("microphone", "systemAudio", "network", "optionalFeatures", "sound", "startup").
#[tauri::command]
pub fn open_system_settings(app: AppHandle, topic: String) -> CmdResult<()> {
    let url = crate::system::settings_url(&topic).ok_or_else(|| invalid("topic"))?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| EngineError::Internal(e.to_string()).into())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioApps {
    /// Per-app capture works on this computer.
    pub supported: bool,
    /// Executable names of apps with audio sessions, playing ones first.
    pub apps: Vec<AudioApp>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioApp {
    pub process: String,
    pub active: bool,
}

#[tauri::command]
pub async fn list_audio_apps() -> CmdResult<AudioApps> {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(windows)]
        return AudioApps {
            supported: sp_audio_io::wasapi_process::process_loopback_supported(),
            apps: sp_audio_io::wasapi_process::audio_apps()
                .into_iter()
                .map(|a| AudioApp {
                    process: a.process,
                    active: a.active,
                })
                .collect(),
        };
        #[allow(unreachable_code)]
        AudioApps {
            supported: false,
            apps: Vec::new(),
        }
    })
    .await
    .map_err(|e| EngineError::Internal(e.to_string()).into())
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    if !url.starts_with("https://") {
        return Err(invalid("url"));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| EngineError::Internal(e.to_string()).into())
}
