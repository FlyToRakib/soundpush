// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod hooks;
mod power;
mod tray;
mod virtual_mic;

use std::path::PathBuf;
use std::sync::Arc;

use sp_engine::{EngineConfig, EngineHandle, EngineState};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tracing::{error, info, warn};

pub struct AppState {
    /// Set once the engine has started (in the background, so the window never waits on it).
    pub engine: std::sync::OnceLock<EngineHandle>,
    pub log_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn engine(&self) -> Result<&EngineHandle, commands::CommandError> {
        self.engine.get().ok_or_else(|| sp_engine::EngineError::Starting.into())
    }
}

pub const MAIN_WINDOW: &str = "main";

fn init_logging(dir: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    std::fs::create_dir_all(dir).ok()?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("soundpush")
        .filename_suffix("log")
        .max_log_files(5)
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .init();
    Some(guard)
}

/// Show the main window, creating it if it was destroyed when closed.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
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
        .theme(theme)
        .build()
    {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(e) => error!(error = %e, "failed to open window"),
    }
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
    info!(version = env!("CARGO_PKG_VERSION"), "SoundPush starting");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let hooks = Arc::new(hooks::DesktopHooks::new(handle.clone(), data_dir.clone()));
            app.manage(AppState {
                engine: std::sync::OnceLock::new(),
                log_dir: log_dir.clone(),
                data_dir: data_dir.clone(),
            });
            tray::create(app)?;

            // Show the window right away; the UI displays "Starting…" until the engine is ready.
            let autostarted = std::env::args().any(|a| a == "--autostart");
            if !autostarted {
                show_main_window(&handle);
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
                            let settings = engine.state().settings.clone();
                            sync_autostart(&handle, settings.desktop.launch_at_login);
                            if autostarted && !settings.desktop.start_minimized {
                                show_main_window(&handle);
                            }
                            let _ = handle.emit("engine://state", &*engine.state());
                            forward_state(handle, engine, hooks);
                        }
                        Err(e) => {
                            error!(error = %e, "engine failed to start");
                            let _ = handle.emit("engine://error", e.to_string());
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
            commands::update_settings,
            commands::dismiss_notice,
            commands::export_diagnostics,
            commands::open_logs_folder,
            commands::open_url,
            commands::virtual_mic_status,
            commands::install_virtual_mic,
            commands::uninstall_virtual_mic,
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
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { .. },
            ..
        } if label == MAIN_WINDOW => {
            let close_to_tray = app
                .try_state::<AppState>()
                .and_then(|s| s.engine.get().map(|e| e.state().settings.desktop.close_to_tray))
                .unwrap_or(true);
            if !close_to_tray {
                app.exit(0);
            }
            // Otherwise the window closes and its webview is destroyed, freeing its memory.
        }
        _ => {}
    });
}

/// Push engine state to the UI and tray; apply desktop-side settings.
fn forward_state(app: AppHandle, engine: EngineHandle, hooks: Arc<hooks::DesktopHooks>) {
    let mut rx = engine.subscribe();
    tauri::async_runtime::spawn(async move {
        let mut last_autostart = None;
        let mut last_theme = None;
        let mut last_streaming = false;
        while rx.changed().await.is_ok() {
            let state = rx.borrow_and_update().clone();

            tray::update(&app, &state);
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
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
        }
    });
}

fn sync_autostart(app: &AppHandle, enabled: bool) {
    let launcher = app.autolaunch();
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
