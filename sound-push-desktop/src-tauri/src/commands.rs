//! Tauri commands: thin wrappers over the engine API.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sp_engine::settings::Settings;
use sp_engine::state::RouteKind;
use sp_engine::{
    DeviceProfile, EngineError, EngineState, ErrorView, NetworkReport, PermissionKind, Policy,
    Severity,
};
use tauri::{AppHandle, Manager, State};
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
    /// An error the engine does not know about (desktop-only features), by i18n key. These carry
    /// no support code: the engine owns that list (docs/error-codes.md).
    fn keyed(key: &'static str, message: impl Into<String>) -> Self {
        Self(ErrorView {
            code: "",
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
pub fn confirm_pairing(
    state: State<'_, AppState>,
    device_id: String,
    accept: bool,
) -> CmdResult<()> {
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
pub fn set_device_blocked(
    state: State<'_, AppState>,
    device_id: String,
    blocked: bool,
) -> CmdResult<()> {
    Ok(state.engine()?.set_device_blocked(device_id, blocked)?)
}

#[tauri::command]
pub fn rename_device(
    state: State<'_, AppState>,
    device_id: String,
    alias: Option<String>,
) -> CmdResult<()> {
    Ok(state.engine()?.rename_device(device_id, alias)?)
}

#[tauri::command]
pub fn set_auto_connect(
    state: State<'_, AppState>,
    device_id: String,
    enabled: bool,
) -> CmdResult<()> {
    Ok(state.engine()?.set_auto_connect(device_id, enabled)?)
}

#[tauri::command]
pub fn set_permission(
    state: State<'_, AppState>,
    device_id: String,
    kind: String,
    policy: String,
) -> CmdResult<()> {
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
pub fn set_device_profile(
    state: State<'_, AppState>,
    device_id: String,
    profile: Option<DeviceProfile>,
) -> CmdResult<()> {
    Ok(state.engine()?.set_device_profile(device_id, profile)?)
}

/// About ten seconds; progress is also published in the engine state.
#[tauri::command]
pub async fn run_network_test(
    state: State<'_, AppState>,
    device_id: String,
) -> CmdResult<NetworkReport> {
    Ok(state.engine()?.run_network_test(device_id).await?)
}

#[tauri::command]
pub fn cancel_network_test(state: State<'_, AppState>, device_id: String) -> CmdResult<()> {
    Ok(state.engine()?.cancel_network_test(device_id)?)
}

/// adb availability and connected phones. Runs adb, so off the UI thread.
#[tauri::command]
pub async fn usb_status(state: State<'_, AppState>) -> CmdResult<crate::usb::UsbStatus> {
    let tcp_port = state.engine()?.state().local.tcp_port;
    tauri::async_runtime::spawn_blocking(move || crate::usb::status(tcp_port))
        .await
        .map_err(|e| EngineError::Internal(e.to_string()).into())
}

/// Forward the phone's SoundPush port to this computer over USB (`adb reverse`).
#[tauri::command]
pub async fn usb_connect(state: State<'_, AppState>, serial: String) -> CmdResult<()> {
    let engine = state.engine()?.clone();
    let local = engine.state().local.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::usb::connect(&serial, local.port, local.tcp_port)
    })
    .await
    .map_err(|e| EngineError::Internal(e.to_string()))?
    .map_err(EngineError::Internal)?;
    // Retry now instead of waiting for the next reconnect attempt.
    Ok(engine.network_changed()?)
}

#[tauri::command]
/// `replace` takes the virtual microphone away from the device feeding it now; without it a
/// second microphone route fails with `error.audio.virtualMicBusy` so the UI can ask first.
pub async fn start_route(
    state: State<'_, AppState>,
    device_id: String,
    kind: RouteKind,
    replace: Option<bool>,
) -> CmdResult<String> {
    Ok(state
        .engine()?
        .start_route(device_id, kind, replace.unwrap_or(false))
        .await?)
}

#[tauri::command]
pub fn stop_route(state: State<'_, AppState>, route_id: String) -> CmdResult<()> {
    Ok(state.engine()?.stop_route(route_id)?)
}

#[tauri::command]
pub fn set_route_volume(
    state: State<'_, AppState>,
    route_id: String,
    volume: f32,
) -> CmdResult<()> {
    Ok(state.engine()?.set_route_volume(route_id, volume)?)
}

#[tauri::command]
pub fn set_route_muted(state: State<'_, AppState>, route_id: String, muted: bool) -> CmdResult<()> {
    Ok(state.engine()?.set_route_muted(route_id, muted)?)
}

#[tauri::command]
pub fn set_route_keep_running(
    state: State<'_, AppState>,
    route_id: String,
    keep: bool,
) -> CmdResult<()> {
    Ok(state.engine()?.set_route_keep_running(route_id, keep)?)
}

#[tauri::command]
pub fn set_peer_speakers_muted(
    state: State<'_, AppState>,
    device_id: String,
    muted: bool,
) -> CmdResult<()> {
    Ok(state.engine()?.set_peer_speakers_muted(device_id, muted)?)
}

#[tauri::command]
pub fn respond_route_request(
    state: State<'_, AppState>,
    request_id: u64,
    accept: bool,
    remember: bool,
) -> CmdResult<()> {
    Ok(state
        .engine()?
        .respond_route_request(request_id, accept, remember)?)
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

/// "Test tone" on the Audio page (plan §5.1): a short chime on the output streams play through,
/// so the user can tell whether they picked the right device. Uses the same tone generator as the
/// accessibility cues.
#[tauri::command]
pub fn play_test_tone(state: State<'_, AppState>) -> CmdResult<()> {
    let target = match state.engine()?.state().settings.output.device.clone() {
        Some(device) => sp_engine::sp_audio_io::RenderTarget::Output(device),
        None => sp_engine::sp_audio_io::RenderTarget::DefaultOutput,
    };
    crate::cues::play_on(state.hooks.backend(), vec![crate::cues::Cue::Test], target);
    Ok(())
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings: Settings,
) -> CmdResult<Settings> {
    Ok(state.engine()?.update_settings(settings).await?)
}

#[tauri::command]
pub fn dismiss_notice(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    Ok(state.engine()?.dismiss_notice(id)?)
}

/// The local security log, newest first (Settings → Privacy & security).
#[tauri::command]
pub async fn get_audit_log(state: State<'_, AppState>) -> CmdResult<Vec<sp_engine::AuditEntry>> {
    Ok(state.engine()?.audit_log().await?)
}

#[tauri::command]
pub async fn clear_audit_log(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(state.engine()?.clear_audit_log().await?)
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
    tauri::async_runtime::spawn_blocking(move || {
        let report = build_diagnostics(&snapshot, &data_dir, &log_dir);
        let dir = data_dir.join("diagnostics");
        std::fs::create_dir_all(&dir).map_err(|e| EngineError::Storage(e.to_string()))?;
        let path = dir.join(format!("soundpush-diagnostics-{}.txt", unix_now()));
        std::fs::write(&path, report.text).map_err(|e| EngineError::Storage(e.to_string()))?;
        Ok(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| EngineError::Internal(e.to_string()))?
}

/// What an export would contain, shown before anything is written (plan §28.2 "User previews
/// contents").
#[tauri::command]
pub async fn preview_diagnostics(state: State<'_, AppState>) -> CmdResult<DiagnosticsPreview> {
    let snapshot = state.engine()?.state();
    let data_dir = state.data_dir.clone();
    let log_dir = state.log_dir.clone();
    tauri::async_runtime::spawn_blocking(move || build_diagnostics(&snapshot, &data_dir, &log_dir))
        .await
        .map_err(|e| EngineError::Internal(e.to_string()).into())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSection {
    /// "system", "state", "crashes" or "log" (UI text keys).
    pub id: &'static str,
    /// Devices, crash reports or log lines included.
    pub count: usize,
    /// What was removed: "addresses", "deviceIds", "pairingCode".
    pub redacted: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsPreview {
    pub sections: Vec<DiagnosticsSection>,
    /// Exactly the text an export writes.
    pub text: String,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn build_diagnostics(
    snapshot: &EngineState,
    data_dir: &std::path::Path,
    log_dir: &std::path::Path,
) -> DiagnosticsPreview {
    let mut report = String::new();
    report.push_str(&format!(
        "SoundPush {} diagnostics\nOS: {} {}\nGenerated: {}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        unix_now()
    ));
    // Addresses (including host names) and the pairing secret go; device ids are shortened
    // by `redact`, and the local device keeps only its display code.
    let mut redacted = snapshot.clone();
    let hide = |values: &mut Vec<String>| {
        for v in values.iter_mut() {
            *v = "<redacted>".to_string();
        }
    };
    for peer in &mut redacted.peers {
        hide(&mut peer.addresses);
        if !peer.remote_address.is_empty() {
            peer.remote_address = "<redacted>".to_string();
        }
    }
    hide(&mut redacted.local.addresses);
    if redacted.pairing.qr_uri.is_some() {
        redacted.pairing.qr_uri = Some("<redacted>".to_string());
    }
    report.push_str("== State ==\n");
    report.push_str(&redact(
        &serde_json::to_string_pretty(&redacted).unwrap_or_default(),
    ));
    let mut sections = vec![
        DiagnosticsSection {
            id: "system",
            count: 0,
            redacted: Vec::new(),
        },
        DiagnosticsSection {
            id: "state",
            count: snapshot.peers.len(),
            redacted: vec!["addresses", "deviceIds", "pairingCode"],
        },
    ];

    // Written by the panic hook, already redacted; stored locally only.
    let crashes = sp_engine::crash::recent(data_dir, 5);
    if !crashes.is_empty() {
        report.push_str("\n\n== Crash reports ==\n");
        sections.push(DiagnosticsSection {
            id: "crashes",
            count: crashes.len(),
            redacted: Vec::new(),
        });
        for (name, contents) in crashes {
            report.push_str(&format!("--- {name}\n{contents}\n"));
        }
    }

    report.push_str("\n\n== Recent log ==\n");
    let mut log_lines = 0;
    if let Some(file) = latest_log(log_dir).and_then(|p| std::fs::File::open(p).ok()) {
        let lines: Vec<String> = BufReader::new(file).lines().map_while(Result::ok).collect();
        for line in lines.iter().skip(lines.len().saturating_sub(2000)) {
            report.push_str(&redact(line));
            report.push('\n');
            log_lines += 1;
        }
    }
    sections.push(DiagnosticsSection {
        id: "log",
        count: log_lines,
        redacted: vec!["addresses", "deviceIds"],
    });
    DiagnosticsPreview {
        sections,
        text: report,
    }
}

/// Replace IP addresses (with or without port and zone) by `<address>` and shorten device ids
/// (32 or more hex digits) to their first 8 digits. Only whole words count, so module paths
/// such as `sp_engine::actor` and timestamps stay intact.
fn redact(text: &str) -> String {
    let in_token = |b: u8| b.is_ascii_hexdigit() || matches!(b, b'.' | b':' | b'[' | b']' | b'%');
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if !in_token(bytes[i]) || (i > 0 && word(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        // An IPv6 zone may be a name ("%wlan0").
        let mut zone = false;
        while i < bytes.len() && (in_token(bytes[i]) || (zone && word(bytes[i]))) {
            match bytes[i] {
                b'%' => zone = true,
                b']' => zone = false,
                _ => {}
            }
            i += 1;
        }
        if i < bytes.len() && word(bytes[i]) {
            continue;
        }
        // Sentence punctuation after an address is not part of it.
        let token = text[start..i].trim_end_matches(['.', ':']);
        let end = start + token.len();
        let replacement = if is_address(token) {
            Some("<address>".to_string())
        } else if token.len() >= 32 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
            Some(token[..8].to_string())
        } else {
            None
        };
        if let Some(replacement) = replacement {
            out.push_str(&text[copied..start]);
            out.push_str(&replacement);
            copied = end;
        }
    }
    out.push_str(&text[copied..]);
    out
}

fn is_address(token: &str) -> bool {
    if token.parse::<std::net::SocketAddr>().is_ok() {
        return true;
    }
    // "[fe80::1%3]:47650", "fe80::1%wlan0" (zone cut at the first % or ]).
    let bare = token.trim_start_matches('[');
    let bare = bare.split(['%', ']']).next().unwrap_or(bare);
    match bare.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(_)) => true,
        Ok(std::net::IpAddr::V6(_)) => bare.len() > 2,
        Err(_) => false,
    }
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

async fn change_virtual_mic(
    f: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> CmdResult<()> {
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
    let accelerator = accelerator
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty());
    {
        // Registration waits on the main thread, so it must not run on it (or on the async runtime).
        let accelerator = accelerator.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::hotkeys::set(&app, kind, accelerator.as_deref())
        })
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
pub async fn network_status(
    state: State<'_, AppState>,
) -> CmdResult<crate::network::NetworkStatus> {
    let status = tauri::async_runtime::spawn_blocking(crate::network::status)
        .await
        .map_err(|e| EngineError::Internal(e.to_string()))?;
    state.hooks.set_network_status(status.clone());
    Ok(status)
}

/// Add SoundPush's firewall rule — and, when asked, move the connected public network to the
/// Private profile — through one UAC prompt, then check again.
#[tauri::command]
pub async fn fix_firewall(
    state: State<'_, AppState>,
    include_public: bool,
    make_private: bool,
) -> CmdResult<crate::network::NetworkStatus> {
    let status = tauri::async_runtime::spawn_blocking(move || {
        crate::network::fix_firewall(include_public, make_private)?;
        // Windows publishes a new network profile a beat after it is set.
        Ok::<_, String>(crate::network::settled_status())
    })
    .await
    .map_err(|e| EngineError::Internal(e.to_string()))?
    .map_err(EngineError::Internal)?;
    state.hooks.set_network_status(status.clone());
    Ok(status)
}

/// USB tethering to a phone, the devices reached through it, and whether it carries this
/// computer's internet (plan §8.1). Lists network adapters, so off the UI thread.
#[tauri::command]
pub async fn tethering_status(
    state: State<'_, AppState>,
) -> CmdResult<crate::tethering::TetheringStatus> {
    let peers: Vec<(String, std::net::IpAddr)> = state
        .engine()?
        .state()
        .peers
        .iter()
        .filter_map(|p| {
            let addr: std::net::SocketAddr = p.remote_address.parse().ok()?;
            Some((p.device_id.clone(), addr.ip()))
        })
        .collect();
    tauri::async_runtime::spawn_blocking(move || crate::tethering::status(&peers))
        .await
        .map_err(|e| EngineError::Internal(e.to_string()).into())
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
        if let Ok(stream) = backend.open_capture(
            &sp_audio_io::CaptureSource::DefaultInput,
            1,
            Box::new(|_| {}),
            Box::new(|_| {}),
        ) {
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
        // Streams on the sound server, moved to a private sink while capturing.
        #[cfg(target_os = "linux")]
        return AudioApps {
            supported: sp_audio_io::pulse::available(),
            apps: sp_audio_io::pulse::audio_apps()
                .unwrap_or_default()
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

/// Close the main window after the one-time "still running" hint: into the tray, or minimized
/// on desktops without a tray, where a closed window would leave nothing to click.
#[tauri::command]
pub async fn close_main_window(app: AppHandle) {
    let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) else {
        return;
    };
    if crate::tray::available(&app) {
        if let Some(saved) = app.try_state::<crate::window_state::WindowState>() {
            saved.save();
        }
        let _ = window.destroy();
    } else {
        let _ = window.minimize();
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
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

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn diagnostics_hide_addresses_and_shorten_device_ids() {
        assert_eq!(
            redact("dialing 192.168.1.20:47650 and 10.0.0.2."),
            "dialing <address> and <address>."
        );
        assert_eq!(
            redact("peer [fe80::1c2b:3d4e%12]:47650 via fe80::1%wlan0, \"::1\""),
            "peer <address> via <address>, \"<address>\""
        );
        assert_eq!(
            redact("\"deviceId\": \"0123456789abcdef0123456789abcdef\""),
            "\"deviceId\": \"01234567\""
        );
        let untouched =
            "2026-09-14T15:35:38.123Z INFO sound_push_desktop::os_events: v0.1.0 took 12ms at ::";
        assert_eq!(redact(untouched), untouched);
        assert_eq!(redact("SP-0000-0000 · déjà 1.2"), "SP-0000-0000 · déjà 1.2");
    }
}

/// Rolling GitHub release holding one updater manifest per channel (`.github/workflows/update-channels.yml`).
const UPDATE_CHANNELS: &str = "https://github.com/FlyToRakib/soundpush/releases/download/updates";
/// Manifest attached to the newest published stable release; read by 0.1 installs and used as a fallback.
const LATEST_STABLE: &str =
    "https://github.com/FlyToRakib/soundpush/releases/latest/download/latest.json";

/// Manifest URLs for a channel, tried in order until one answers.
fn update_endpoints(channel: sp_engine::settings::UpdateChannel) -> Vec<String> {
    use sp_engine::settings::UpdateChannel;
    match channel {
        UpdateChannel::Stable => vec![
            format!("{UPDATE_CHANNELS}/stable.json"),
            LATEST_STABLE.to_string(),
        ],
        UpdateChannel::Beta => vec![
            format!("{UPDATE_CHANNELS}/beta.json"),
            format!("{UPDATE_CHANNELS}/stable.json"),
            LATEST_STABLE.to_string(),
        ],
    }
}

/// Same shape as the updater plugin's own `check` result, so the UI wraps it in the plugin's `Update`
/// class and downloads/installs through the plugin (which verifies the minisign signature).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    rid: tauri::ResourceId,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    raw_json: serde_json::Value,
}

/// Looks for an update on `channel`. Staged rollout is applied by the UI from `rawJson.rollout`.
#[tauri::command]
pub async fn check_update(
    webview: tauri::Webview,
    channel: sp_engine::settings::UpdateChannel,
    timeout_ms: Option<u64>,
) -> CmdResult<Option<UpdateMetadata>> {
    use tauri::Manager;
    use tauri_plugin_updater::UpdaterExt;

    // Flatpak installs are updated by Flatpak/Flathub, never by the app itself.
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Ok(None);
    }

    let failed =
        |e: &dyn std::fmt::Display| CommandError::keyed("update.checkFailed", e.to_string());
    let endpoints = update_endpoints(channel)
        .iter()
        .map(|u| tauri::Url::parse(u))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| failed(&e))?;
    let mut builder = webview
        .updater_builder()
        .endpoints(endpoints)
        .map_err(|e| failed(&e))?;
    if let Some(ms) = timeout_ms {
        builder = builder.timeout(std::time::Duration::from_millis(ms));
    }
    let update = builder
        .build()
        .map_err(|e| failed(&e))?
        .check()
        .await
        .map_err(|e| failed(&e))?;
    Ok(update.map(|update| UpdateMetadata {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        date: update
            .raw_json
            .get("pub_date")
            .and_then(|d| d.as_str())
            .map(str::to_string),
        body: update.body.clone(),
        raw_json: update.raw_json.clone(),
        rid: webview.resources_table().add(update),
    }))
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use sp_engine::settings::UpdateChannel;

    #[test]
    fn beta_falls_back_to_stable_manifests() {
        let stable = update_endpoints(UpdateChannel::Stable);
        let beta = update_endpoints(UpdateChannel::Beta);
        assert!(stable[0].ends_with("/stable.json"));
        assert!(beta[0].ends_with("/beta.json"));
        assert_eq!(&beta[1..], &stable[..]);
        for url in stable.iter().chain(&beta) {
            assert!(tauri::Url::parse(url).is_ok(), "{url}");
        }
    }
}
