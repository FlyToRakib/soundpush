// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod crash_dump;
mod cues;
mod default_devices;
mod device_watch;
mod hooks;
mod hotkeys;
#[cfg(target_os = "macos")]
mod macos;
mod network;
mod os_events;
#[cfg(target_os = "linux")]
mod portals;
mod power;
mod system;
mod tethering;
mod tray;
mod usb;
mod virtual_mic;
mod webview2;
mod window_state;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sp_engine::{EngineConfig, EngineHandle, EngineState};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tracing::{error, info, warn};

pub struct AppState {
    /// Set once the engine has started (in the background, so the window never waits on it).
    pub engine: std::sync::OnceLock<EngineHandle>,
    /// Why the engine could not start; the UI shows it instead of "Starting…" forever.
    pub start_error: std::sync::Mutex<Option<String>>,
    pub log_dir: PathBuf,
    pub data_dir: PathBuf,
    pub hooks: Arc<hooks::DesktopHooks>,
}

impl AppState {
    pub fn engine(&self) -> Result<&EngineHandle, commands::CommandError> {
        self.engine
            .get()
            .ok_or_else(|| sp_engine::EngineError::Starting.into())
    }
}

pub const MAIN_WINDOW: &str = "main";
/// How long a normal launch waits for the engine before showing the window with "Starting…".
const ENGINE_WAIT: Duration = Duration::from_millis(1500);
/// Settings tip id recorded once the "SoundPush keeps running" hint was shown.
const CLOSE_HINT_TIP: &str = "closeToTray";

/// Whether the OS started SoundPush at sign-in rather than the user opening it. The update check
/// waits half a minute in that case (plan §24), so signing in stays quiet.
static AUTOSTARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn autostarted() -> bool {
    AUTOSTARTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Size-rotated log files, 5 × 10 MB (plan §28.1). The engine raises the level to `debug` while
/// the "Debug logging" setting is on.
fn init_logging(dir: &std::path::Path) -> Option<sp_engine::logging::LogGuard> {
    sp_engine::logging::init_files(sp_engine::logging::LogConfig::desktop(dir.to_path_buf()))
}

/// Show the main window, creating it if it was destroyed when closed.
pub fn show_main_window(app: &AppHandle) {
    // Several threads may ask at once (first launch, engine failure, tray); create one window.
    static OPENING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _opening = OPENING.lock();
    // Building the window would fail with a bare "webview error"; check first so the user is
    // told what to install (plan §10.3). Nothing else in SoundPush needs the webview.
    if !webview2::available() {
        warn!("no WebView2 runtime; the window cannot open");
        missing_webview(app);
        return;
    }
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        // A window kept in memory received no snapshots while it was hidden (see
        // `forward_state`), so it starts from the current one.
        if let Some(engine) = app
            .try_state::<AppState>()
            .and_then(|s| s.engine.get().cloned())
        {
            let _ = app.emit("engine://state", &*engine.state());
        }
        return;
    }
    let theme = app
        .try_state::<AppState>()
        .and_then(|s| s.engine.get().map(|e| window_theme(&e.state())))
        .unwrap_or(None);
    match WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title("SoundPush")
        .inner_size(1000.0, 700.0)
        .min_inner_size(760.0, 520.0)
        .center()
        // Sized and placed like last time before it appears.
        .visible(false)
        // Ctrl/Cmd + and − scale the whole UI (text scaling for low vision).
        .zoom_hotkeys_enabled(true)
        .theme(theme)
        .build()
    {
        Ok(window) => {
            if let Some(saved) = app.try_state::<window_state::WindowState>() {
                saved.restore(&window);
            }
            let _ = window.show();
            let _ = window.set_focus();
        }
        Err(e) => {
            error!(error = %e, "failed to open window");
            // Windows: almost always a missing or damaged WebView2 runtime. Say so and offer the
            // installer instead of leaving the user with nothing (plan §10.3, G9).
            missing_webview(app);
        }
    }
}

/// Explain a webview that will not start, at most once per run. The engine and the tray keep
/// working without it, so this never stops SoundPush.
fn missing_webview(app: &AppHandle) {
    static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if ASKED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    tray::show_error(
        app,
        "The interface could not open; SoundPush is still running.",
    );
    webview2::offer_install(app);
}

fn window_theme(state: &EngineState) -> Option<tauri::Theme> {
    match state.settings.theme {
        sp_engine::settings::Theme::Light => Some(tauri::Theme::Light),
        sp_engine::settings::Theme::Dark => Some(tauri::Theme::Dark),
        sp_engine::settings::Theme::System => None,
    }
}

fn main() {
    let data_dir = hooks::data_dir();
    let log_dir = data_dir.join("logs");
    let _log_guard = init_logging(&log_dir);
    // Panics leave a redacted report in the data folder (never uploaded); the engine mentions it
    // once on the next start and diagnostics exports include it. A crash the panic hook cannot
    // see (access violation, SIGSEGV in a driver) leaves a note and a local minidump instead.
    sp_engine::crash::install(&data_dir, env!("CARGO_PKG_VERSION"));
    crash_dump::install(&data_dir, env!("CARGO_PKG_VERSION"));
    info!(version = env!("CARGO_PKG_VERSION"), "SoundPush starting");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_opener::init())
        // Update manifests are verified against the public key in tauri.conf.json.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(hotkeys::plugin())
        .setup(move |app| {
            let handle = app.handle().clone();
            let hooks = Arc::new(hooks::DesktopHooks::new(handle.clone(), data_dir.clone()));
            app.manage(AppState {
                engine: std::sync::OnceLock::new(),
                start_error: std::sync::Mutex::new(None),
                log_dir: log_dir.clone(),
                data_dir: data_dir.clone(),
                hooks: hooks.clone(),
            });
            app.manage(hotkeys::Hotkeys::default());
            app.manage(window_state::WindowState::load(&data_dir));
            app.manage(Arc::new(default_devices::DefaultDevices::new(&data_dir)));
            if let Err(e) = tray::create(app) {
                // A desktop without tray support must not stop SoundPush; the window stays reachable.
                warn!(error = %e, "could not create the tray icon");
            }
            device_watch::start(handle.clone());
            watch_network(hooks.clone());
            os_events::start(handle.clone(), hooks.clone());
            os_events::register_restart();
            virtual_mic::restore();

            // The engine starts first (plan §13.1): a normal launch opens the window once it is
            // ready, or after ENGINE_WAIT with "Starting…" when the OS holds it up (Keychain prompt).
            let autostarted = std::env::args().any(|a| a == "--autostart");
            // A macOS login item (SMAppService) starts without arguments.
            #[cfg(target_os = "macos")]
            let autostarted = autostarted || macos::launched_at_login();
            AUTOSTARTED.store(autostarted, std::sync::atomic::Ordering::Relaxed);
            let (ready, engine_ready) = std::sync::mpsc::channel::<()>();
            if !autostarted {
                let handle = handle.clone();
                std::thread::Builder::new()
                    .name("sp-first-window".into())
                    .spawn(move || {
                        let _ = engine_ready.recv_timeout(ENGINE_WAIT);
                        show_main_window(&handle);
                    })?;
            }

            // The engine may wait on the OS (e.g. a Keychain prompt), so never start it on the main thread.
            std::thread::Builder::new()
                .name("sp-engine-start".into())
                .spawn(move || {
                    let config = EngineConfig {
                        app_version: env!("CARGO_PKG_VERSION").to_string(),
                        ..EngineConfig::default()
                    };
                    match EngineHandle::start(hooks.clone(), config) {
                        Ok(engine) => {
                            if let Some(state) = handle.try_state::<AppState>() {
                                let _ = state.engine.set(engine.clone());
                            }
                            let _ = ready.send(());
                            let settings = engine.state().settings.clone();
                            // Sandboxed and Wayland desktops hand the shortcuts to the portal
                            // before any window exists (plan §26.2).
                            #[cfg(target_os = "linux")]
                            hotkeys::use_portal(
                                &handle,
                                settings.desktop.mute_hotkey.as_deref(),
                                settings.desktop.push_to_talk_hotkey.as_deref(),
                            );
                            sync_autostart(&handle, settings.desktop.launch_at_login);
                            if autostarted {
                                // Plan §24: after sign-in the window opens only when asked to or
                                // while onboarding is unfinished.
                                let onboarded =
                                    settings.dismissed_tips.iter().any(|t| t == "onboarding");
                                if !settings.desktop.start_minimized || !onboarded {
                                    show_main_window(&handle);
                                } else if !tray::available(&handle) {
                                    // No tray on this desktop: a minimized window keeps SoundPush reachable.
                                    show_main_window(&handle);
                                    if let Some(window) = handle.get_webview_window(MAIN_WINDOW) {
                                        let _ = window.minimize();
                                    }
                                }
                            }
                            let _ = handle.emit("engine://state", &*engine.state());
                            forward_state(handle, engine, hooks);
                        }
                        Err(e) => {
                            error!(error = %e, "engine failed to start");
                            if let Some(state) = handle.try_state::<AppState>()
                                && let Ok(mut slot) = state.start_error.lock()
                            {
                                *slot = Some(e.to_string());
                            }
                            let _ = handle.emit("engine://error", e.to_string());
                            let _ = ready.send(());
                            tray::show_error(&handle, &e.to_string());
                            // Autostarted in the tray: a failure must not stay invisible.
                            show_main_window(&handle);
                        }
                    }
                })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::start_pairing,
            commands::stop_pairing,
            commands::pair_with_address,
            commands::pair_with_device,
            commands::pair_with_qr,
            commands::confirm_pairing,
            commands::connect_device,
            commands::disconnect_device,
            commands::forget_device,
            commands::set_device_blocked,
            commands::rename_device,
            commands::set_auto_connect,
            commands::set_permission,
            commands::set_device_profile,
            commands::run_network_test,
            commands::cancel_network_test,
            commands::usb_status,
            commands::usb_connect,
            commands::start_route,
            commands::stop_route,
            commands::set_route_volume,
            commands::set_route_muted,
            commands::set_route_keep_running,
            commands::set_peer_speakers_muted,
            commands::respond_route_request,
            commands::set_mic_muted,
            commands::set_mic_monitor,
            commands::refresh_audio_devices,
            commands::play_test_tone,
            commands::update_settings,
            commands::dismiss_notice,
            commands::get_start_error,
            commands::export_diagnostics,
            commands::preview_diagnostics,
            commands::get_audit_log,
            commands::clear_audit_log,
            commands::open_logs_folder,
            commands::open_url,
            commands::check_update,
            commands::virtual_mic_status,
            commands::install_virtual_mic,
            commands::uninstall_virtual_mic,
            commands::restart_computer,
            commands::hotkey_status,
            commands::set_hotkey,
            commands::network_status,
            commands::fix_firewall,
            commands::system_status,
            commands::update_conditions,
            commands::request_microphone,
            commands::open_system_settings,
            commands::list_audio_apps,
            commands::tethering_status,
            commands::close_main_window,
            commands::quit_app,
        ])
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(e) => {
            error!(error = %e, "failed to start");
            eprintln!("SoundPush failed to start: {e}");
            std::process::exit(1);
        }
    };

    app.run(|app, event| match event {
        // Keep running in the tray when the last window closes; explicit quits pass an exit code.
        RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() {
                api.prevent_exit();
            }
        }
        // Tauri ends the process right after this without dropping managed state, so the engine
        // must stop here: unmute the speakers, tell peers, release sleep prevention.
        RunEvent::Exit => {
            if let Some(saved) = app.try_state::<window_state::WindowState>() {
                saved.save();
            }
            // Give the computer its own default input and output back before leaving.
            if let Some(devices) = app.try_state::<Arc<default_devices::DefaultDevices>>() {
                devices.restore();
            }
            if let Some(engine) = app
                .try_state::<AppState>()
                .and_then(|s| s.engine.get().cloned())
            {
                engine.shutdown(std::time::Duration::from_secs(2));
            }
        }
        RunEvent::WindowEvent { label, event, .. } if label == MAIN_WINDOW => match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if let (Some(saved), Some(window)) = (
                    app.try_state::<window_state::WindowState>(),
                    app.get_webview_window(MAIN_WINDOW),
                ) {
                    saved.track(&window);
                }
            }
            WindowEvent::CloseRequested { api, .. } => on_close_requested(app, &api),
            _ => {}
        },
        _ => {}
    });
}

/// "Keep window in memory for instant reopen" (plan §13.1), off by default.
pub fn keeps_window_in_memory(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|s| s.engine.get().map(|e| e.state()))
        .is_some_and(|s| s.settings.desktop.keep_window_in_memory)
}

/// Closing the window (plan §24): quits when "Keep running when the window is closed" is off.
/// Otherwise the webview is destroyed, freeing its memory, and SoundPush stays in the tray — or,
/// with "Keep window in memory for instant reopen" on, the window is only hidden and keeps its
/// webview loaded. The first time the window stays open for a one-time explanation; on desktops
/// without a tray the window is minimized instead of disappearing.
fn on_close_requested(app: &AppHandle, api: &tauri::CloseRequestApi) {
    if let Some(saved) = app.try_state::<window_state::WindowState>() {
        saved.save();
    }
    let Some(state) = app
        .try_state::<AppState>()
        .and_then(|s| s.engine.get().map(|e| e.state()))
    else {
        return;
    };
    if !state.settings.desktop.close_to_tray {
        app.exit(0);
        return;
    }
    let tray = tray::available(app);
    if !state
        .settings
        .dismissed_tips
        .iter()
        .any(|t| t == CLOSE_HINT_TIP)
    {
        api.prevent_close();
        let _ = app.emit_to(MAIN_WINDOW, "window://close-hint", tray);
    } else if !tray {
        api.prevent_close();
        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
            let _ = window.minimize();
        }
    } else if state.settings.desktop.keep_window_in_memory {
        api.prevent_close();
        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
            let _ = window.hide();
        }
    }
    // Otherwise the close goes ahead and the webview is released with the window.
}

/// Push engine state to the UI and tray; apply desktop-side settings.
fn forward_state(app: AppHandle, engine: EngineHandle, hooks: Arc<hooks::DesktopHooks>) {
    let mut rx = engine.subscribe();
    tauri::async_runtime::spawn(async move {
        let mut last_autostart = None;
        let mut last_theme = None;
        let mut last_streaming = false;
        let mut last_hotkeys = None;
        let mut last_defaults: Option<(Option<String>, Option<String>)> = None;
        let mut previous: Option<Arc<EngineState>> = None;
        while rx.changed().await.is_ok() {
            let state = rx.borrow_and_update().clone();

            tray::update(&app, &state);
            if let Some(previous) = &previous
                && state.settings.audio_cues
            {
                cues::play(hooks.backend(), cues::changes(previous, &state));
            }
            previous = Some(state.clone());

            // Shortcuts are re-registered only when the saved ones change (see hotkeys::set).
            let wanted = (
                state.settings.desktop.mute_hotkey.clone(),
                state.settings.desktop.push_to_talk_hotkey.clone(),
            );
            if last_hotkeys.as_ref() != Some(&wanted) {
                last_hotkeys = Some(wanted.clone());
                let handle = app.clone();
                // Registration waits on the main thread, so it runs off both it and this task.
                tauri::async_runtime::spawn_blocking(move || {
                    hotkeys::apply(&handle, wanted.0.as_deref(), wanted.1.as_deref())
                });
            }
            // Snapshots go to the webview only while a window exists (plan §13.1) and is on
            // screen: a window kept in memory but hidden is caught up by `show_main_window`.
            if let Some(window) = app
                .get_webview_window(MAIN_WINDOW)
                .filter(|w| w.is_visible().unwrap_or(true))
            {
                if let Err(e) = app.emit("engine://state", &*state) {
                    warn!(error = %e, "emit failed");
                }
                let theme = window_theme(&state);
                if last_theme != Some(theme) {
                    let _ = window.set_theme(theme);
                    last_theme = Some(theme);
                }
            } else {
                last_theme = None;
            }

            let autostart = state.settings.desktop.launch_at_login;
            if last_autostart != Some(autostart) {
                if last_autostart.is_some() {
                    sync_autostart(&app, autostart);
                }
                last_autostart = Some(autostart);
            }

            let streaming = state
                .routes
                .iter()
                .any(|r| r.status == sp_engine::state::RouteStatus::Active);
            let prevent = streaming && state.settings.desktop.prevent_sleep_while_streaming;
            if prevent != last_streaming {
                hooks.set_prevent_sleep(prevent);
                last_streaming = prevent;
            }

            // "Make SoundPush the default input and output while active" (plan §23.2). Setting a
            // default takes a moment on every platform, so it runs only when the answer changes.
            let wanted = wanted_defaults(&state);
            if last_defaults.as_ref() != Some(&wanted) {
                last_defaults = Some(wanted.clone());
                if let Some(devices) = app
                    .try_state::<Arc<default_devices::DefaultDevices>>()
                    .map(|s| s.inner().clone())
                {
                    tauri::async_runtime::spawn_blocking(move || {
                        devices.apply(wanted.0.as_deref(), wanted.1.as_deref());
                    });
                }
            }
        }
    });
}

/// Which devices "Make SoundPush the default input and output while active" wants right now
/// (plan §23.2): `(input, output)`, each `None` when SoundPush should not touch that direction.
///
/// The recording default becomes the virtual microphone while the phone feeds it, so apps pick
/// the phone up without being set up one by one. The playback default is only ever moved to a
/// virtual cable SoundPush captures: pointing it at a real speaker would change nothing.
fn wanted_defaults(state: &EngineState) -> (Option<String>, Option<String>) {
    use sp_engine::state::{RouteKind, RouteStatus};
    if !state.settings.desktop.default_devices_while_active {
        return (None, None);
    }
    let active = |kind: RouteKind| {
        state
            .routes
            .iter()
            .any(|r| r.status == RouteStatus::Active && r.kind == kind)
    };
    let input = active(RouteKind::ReceiveMicToVirtualMic)
        .then(|| state.capabilities.virtual_mic_input.clone())
        .flatten();
    let output = active(RouteKind::SendSystemAudio)
        .then(|| state.settings.capture.system_device.clone())
        .flatten()
        .filter(|device| hooks::is_virtual_cable(device));
    (input, output)
}

/// Re-check the firewall and network profile now and every minute, so connection errors can
/// point at the firewall fix (`PlatformHooks::inbound_blocked`).
fn watch_network(hooks: Arc<hooks::DesktopHooks>) {
    if !cfg!(windows) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("sp-network-check".into())
        .spawn(move || {
            loop {
                hooks.set_network_status(network::status());
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        });
    if let Err(e) = spawned {
        warn!(error = %e, "could not start network checks");
    }
}

fn sync_autostart(app: &AppHandle, enabled: bool) {
    // Inside a Flatpak an XDG autostart file written by the app is not seen by the session; the
    // portal writes one outside the sandbox and asks the user once (plan §26.2, §24).
    #[cfg(target_os = "linux")]
    if portals::sandboxed() {
        let command = std::env::current_exe()
            .map(|exe| vec![exe.to_string_lossy().to_string(), "--autostart".into()])
            .unwrap_or_else(|_| vec!["soundpush".into(), "--autostart".into()]);
        match portals::request_background(
            "Keep streaming to your phone while the window is closed",
            enabled,
            &command,
        ) {
            Some(granted) if granted == enabled => return,
            // The user said no, or said no earlier: the setting must show what is really true.
            Some(granted) => {
                warn!(
                    wanted = enabled,
                    granted, "the portal decided about launch at sign-in"
                );
                if let Some(engine) = app
                    .try_state::<AppState>()
                    .and_then(|s| s.engine.get().cloned())
                {
                    let mut settings = engine.state().settings.clone();
                    settings.desktop.launch_at_login = granted;
                    tauri::async_runtime::spawn(async move {
                        let _ = engine.update_settings(settings).await;
                    });
                }
                return;
            }
            // No Background interface: fall through to the ordinary XDG autostart file.
            None => {}
        }
    }
    let launcher = app.autolaunch();
    // macOS 13+: an SMAppService login item. The LaunchAgent that earlier versions wrote through
    // the autostart plugin (~/Library/LaunchAgents/SoundPush.plist) is removed, so SoundPush
    // starts once; the plugin remains for Windows, Linux and older macOS.
    #[cfg(target_os = "macos")]
    if macos::login_items_supported() {
        if launcher.is_enabled().unwrap_or(false)
            && let Err(e) = launcher.disable()
        {
            warn!(error = %e, "could not remove the old launch agent");
        }
        if let Err(e) = macos::set_login_item(enabled) {
            warn!(error = %e, "could not update launch at login");
        }
        return;
    }
    let current = launcher.is_enabled().unwrap_or(false);
    let result = match (enabled, current) {
        (true, false) => launcher.enable(),
        (false, true) => launcher.disable(),
        _ => Ok(()),
    };
    if let Err(e) = result {
        warn!(error = %e, "could not update launch at login");
    }
}
