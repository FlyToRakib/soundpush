//! System tray: status, quick actions, open, quit.

use std::sync::Mutex;

use sp_engine::EngineState;
use sp_engine::state::{RouteKind, RouteStatus};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};

use crate::{AppState, show_main_window};

const TRAY_ID: &str = "main";

struct TrayItems {
    status: MenuItem<Wry>,
    mute: CheckMenuItem<Wry>,
    stop_all: MenuItem<Wry>,
    last_summary: Mutex<String>,
}

pub fn create(app: &mut App) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "SoundPush", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open SoundPush", true, None::<&str>)?;
    let mute = CheckMenuItem::with_id(app, "mute", "Mute microphone", true, false, None::<&str>)?;
    let stop_all = MenuItem::with_id(app, "stop_all", "Stop all streams", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit SoundPush", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &mute,
            &stop_all,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("SoundPush")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "mute" => {
                if let (Some(state), Some(items)) =
                    (app.try_state::<AppState>(), app.try_state::<TrayItems>())
                {
                    let muted = items.mute.is_checked().unwrap_or(false);
                    if let Some(engine) = state.engine.get() {
                        let _ = engine.set_mic_muted(muted);
                    }
                }
            }
            "stop_all" => {
                if let Some(engine) = app
                    .try_state::<AppState>()
                    .and_then(|s| s.engine.get().cloned())
                {
                    for route in engine.state().routes.iter() {
                        let _ = engine.stop_route(route.route_id.clone());
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }
    builder.build(app)?;

    app.manage(TrayItems {
        status,
        mute,
        stop_all,
        last_summary: Mutex::new(String::new()),
    });
    Ok(())
}

pub fn update(app: &AppHandle, state: &EngineState) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let active: Vec<_> = state
        .routes
        .iter()
        .filter(|r| r.status == RouteStatus::Active)
        .collect();
    let mic_live = active.iter().any(|r| {
        matches!(
            r.kind,
            RouteKind::SendMicToSpeaker
                | RouteKind::SendMicToVirtualMic
                | RouteKind::ReceiveMicToVirtualMic
        )
    });
    let summary = match active.len() {
        0 => {
            let connected = state
                .peers
                .iter()
                .filter(|p| {
                    p.trusted && p.connection == sp_engine::state::ConnectionStatus::Connected
                })
                .count();
            if connected == 0 {
                "Not streaming".to_string()
            } else {
                format!("{connected} device(s) connected")
            }
        }
        1 => format!("Streaming with {}", active[0].peer_name),
        n => format!("{n} streams active"),
    };
    let summary = match (mic_live, state.mic_muted) {
        (true, true) => format!("Microphone muted · {summary}"),
        (true, false) => format!("● Microphone live · {summary}"),
        _ => summary,
    };

    // Mute can change from a shortcut or the window, so the check mark follows the engine.
    let key = format!("{summary}|{}|{}", state.mic_muted, mic_live);
    let Ok(mut last) = items.last_summary.lock() else {
        return;
    };
    if *last == key {
        return;
    }
    let _ = items.status.set_text(&summary);
    let _ = items.stop_all.set_enabled(!active.is_empty());
    let _ = items.mute.set_checked(state.mic_muted);
    let _ = items.mute.set_enabled(mic_live || state.mic_muted);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(format!("SoundPush — {summary}")));
    }
    *last = key;
}
