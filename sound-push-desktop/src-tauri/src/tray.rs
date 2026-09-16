//! System tray (plan §13.2, §23.1): status line, active routes with mute and stop, microphone
//! mute with its shortcut, recent devices with quick actions, open, quit. The icon shows idle,
//! streaming, microphone live, attention (a prompt is waiting) or error.
//!
//! The menu is described as plain data and rebuilt only when that description changes, so the
//! once-a-second statistics updates never touch it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use sp_engine::state::{RouteKind, RouteStatus, RouteView};
use sp_engine::{EngineState, Severity};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};
use tracing::warn;

use crate::{AppState, show_main_window};

const TRAY_ID: &str = "main";
/// Recent devices listed in the menu.
const RECENT_DEVICES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    Idle,
    Streaming,
    MicLive,
    Attention,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Text(String),
    Separator,
    Action {
        id: String,
        text: String,
        enabled: bool,
    },
    Check {
        id: String,
        text: String,
        checked: bool,
        enabled: bool,
    },
    Submenu {
        text: String,
        items: Vec<Entry>,
    },
}

struct TrayItems {
    base_icon: Option<Image<'static>>,
    /// Last menu, icon and tooltip applied.
    last: Mutex<Option<(Vec<Entry>, IconState, String)>>,
    /// False on Linux desktops without a StatusNotifier host, where the icon is invisible.
    available: AtomicBool,
}

pub fn create(app: &mut App) -> tauri::Result<()> {
    let base_icon = app.default_window_icon().map(|i| i.clone().to_owned());
    let menu = build_menu(app.handle(), &[Entry::Text("SoundPush".into())])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("SoundPush")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id.as_ref()))
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
    if let Some(icon) = &base_icon {
        builder = builder.icon(icon.clone());
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }
    builder.build(app)?;

    app.manage(TrayItems {
        base_icon,
        last: Mutex::new(None),
        available: AtomicBool::new(true),
    });
    #[cfg(target_os = "linux")]
    detect_status_notifier(app.handle().clone());
    Ok(())
}

/// Whether the tray icon can be seen. Without one, closing the window minimizes it instead.
pub fn available(app: &AppHandle) -> bool {
    app.try_state::<TrayItems>()
        .is_some_and(|t| t.available.load(Ordering::Relaxed))
}

/// GNOME without the AppIndicator extension has no StatusNotifier host. Asked on the session
/// bus off the main thread; assumed present until answered.
#[cfg(target_os = "linux")]
fn detect_status_notifier(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("sp-tray-check".into())
        .spawn(move || {
            let present = dbus::blocking::Connection::new_session()
                .and_then(|c| {
                    c.with_proxy(
                        "org.freedesktop.DBus",
                        "/org/freedesktop/DBus",
                        std::time::Duration::from_secs(2),
                    )
                    .method_call::<(bool,), _, _, _>(
                        "org.freedesktop.DBus",
                        "NameHasOwner",
                        ("org.kde.StatusNotifierWatcher",),
                    )
                })
                .map(|(owned,)| owned)
                .unwrap_or(false);
            if !present {
                warn!("no StatusNotifier host: the tray icon is not shown on this desktop");
                if let Some(items) = app.try_state::<TrayItems>() {
                    items.available.store(false, Ordering::Relaxed);
                }
            }
        });
    if let Err(e) = spawned {
        warn!(error = %e, "could not check for a tray host");
    }
}

fn engine(app: &AppHandle) -> Option<sp_engine::EngineHandle> {
    app.try_state::<AppState>()
        .and_then(|s| s.engine.get().cloned())
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main_window(app),
        "quit" => app.exit(0),
        "mute" => {
            if let Some(engine) = engine(app) {
                let _ = engine.set_mic_muted(!engine.state().mic_muted);
            }
        }
        "stop_all" => {
            if let Some(engine) = engine(app) {
                for route in engine.state().routes.iter() {
                    let _ = engine.stop_route(route.route_id.clone());
                }
            }
        }
        _ => {
            let Some(engine) = engine(app) else {
                return;
            };
            let Some((action, target)) = id.split_once(':') else {
                return;
            };
            let target = target.to_string();
            match action {
                "route-stop" => {
                    let _ = engine.stop_route(target);
                }
                "route-mute" => {
                    let muted = engine
                        .state()
                        .routes
                        .iter()
                        .find(|r| r.route_id == target)
                        .is_some_and(|r| r.muted);
                    let _ = engine.set_route_muted(target, !muted);
                }
                "connect" => {
                    let _ = engine.connect(target);
                }
                "disconnect" => {
                    let _ = engine.disconnect(target);
                }
                "listen" | "microphone" => {
                    let kind = if action == "listen" {
                        RouteKind::SendSystemAudio
                    } else {
                        RouteKind::ReceiveMicToVirtualMic
                    };
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = engine.start_route(target, kind, false).await {
                            warn!(error = %e, "could not start a stream from the tray");
                        }
                    });
                }
                _ => {}
            }
        }
    }
}

/// Follow the engine state. Cheap when nothing in the menu or icon changed.
pub fn update(app: &AppHandle, state: &EngineState) {
    let (entries, icon, tooltip) = describe(state);
    apply(app, entries, icon, tooltip);
}

/// The engine could not start: the tray must not look idle.
pub fn show_error(app: &AppHandle, message: &str) {
    let entries = vec![
        Entry::Text("SoundPush couldn't start".into()),
        Entry::Separator,
        Entry::Action {
            id: "open".into(),
            text: "Open SoundPush".into(),
            enabled: true,
        },
        Entry::Action {
            id: "quit".into(),
            text: "Quit SoundPush".into(),
            enabled: true,
        },
    ];
    apply(
        app,
        entries,
        IconState::Error,
        format!("SoundPush — couldn't start: {message}"),
    );
}

fn apply(app: &AppHandle, entries: Vec<Entry>, icon: IconState, tooltip: String) {
    let (Some(items), Some(tray)) = (app.try_state::<TrayItems>(), app.tray_by_id(TRAY_ID)) else {
        return;
    };
    let Ok(mut last) = items.last.lock() else {
        return;
    };
    let (old_entries, old_icon, old_tooltip) = match last.as_ref() {
        Some((e, i, t)) => (Some(e), Some(*i), Some(t)),
        None => (None, None, None),
    };
    if old_entries != Some(&entries) {
        match build_menu(app, &entries) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => warn!(error = %e, "could not update the tray menu"),
        }
    }
    if old_icon != Some(icon) {
        if let Some(base) = &items.base_icon {
            let rgba = badge(base.rgba(), base.width(), base.height(), icon);
            let _ = tray.set_icon(Some(Image::new_owned(rgba, base.width(), base.height())));
            // The macOS menu bar tints a template image; a colored badge must keep its color.
            #[cfg(target_os = "macos")]
            let _ = tray.set_icon_as_template(icon == IconState::Idle);
        }
    }
    if old_tooltip != Some(&tooltip) {
        let _ = tray.set_tooltip(Some(&tooltip));
    }
    *last = Some((entries, icon, tooltip));
}

fn build_menu(app: &AppHandle, entries: &[Entry]) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    for entry in entries {
        menu.append(build_item(app, entry)?.as_ref())?;
    }
    Ok(menu)
}

fn build_item(app: &AppHandle, entry: &Entry) -> tauri::Result<Box<dyn IsMenuItem<Wry>>> {
    Ok(match entry {
        Entry::Text(text) => Box::new(MenuItem::new(app, text, false, None::<&str>)?),
        Entry::Separator => Box::new(PredefinedMenuItem::separator(app)?),
        Entry::Action { id, text, enabled } => Box::new(MenuItem::with_id(
            app,
            id.as_str(),
            text,
            *enabled,
            None::<&str>,
        )?),
        Entry::Check {
            id,
            text,
            checked,
            enabled,
        } => Box::new(CheckMenuItem::with_id(
            app,
            id.as_str(),
            text,
            *enabled,
            *checked,
            None::<&str>,
        )?),
        Entry::Submenu { text, items } => {
            let submenu = Submenu::new(app, text, true)?;
            for item in items {
                submenu.append(build_item(app, item)?.as_ref())?;
            }
            Box::new(submenu)
        }
    })
}

fn route_title(route: &RouteView) -> String {
    let peer = &route.peer_name;
    let title = match route.kind {
        RouteKind::SendSystemAudio => format!("Computer audio → {peer}"),
        RouteKind::SendAppAudio => format!("App audio → {peer}"),
        RouteKind::SendMicToVirtualMic | RouteKind::SendMicToSpeaker => {
            format!("Microphone → {peer}")
        }
        RouteKind::ReceiveSystemAudio => format!("{peer} audio → this computer"),
        RouteKind::ReceiveAppAudio => format!("{peer} apps → this computer"),
        RouteKind::ReceiveMicToVirtualMic => format!("{peer} microphone → virtual microphone"),
        RouteKind::ReceiveMicToSpeaker => format!("{peer} microphone → speakers"),
    };
    match route.status {
        RouteStatus::Active => title,
        RouteStatus::Paused => format!("{title} (reconnecting)"),
        _ => format!("{title} (starting)"),
    }
}

/// "CommandOrControl+Shift+M" → "Ctrl+Shift+M" (⌘ on macOS).
fn shortcut_label(accelerator: &str) -> String {
    let command = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    accelerator
        .replace("CommandOrControl", command)
        .replace("CmdOrCtrl", command)
}

pub fn icon_state(state: &EngineState) -> IconState {
    let active = state
        .routes
        .iter()
        .filter(|r| r.status == RouteStatus::Active);
    if state.notices.iter().any(|n| n.severity == Severity::Error) {
        IconState::Error
    } else if !state.requests.is_empty() || !state.pairing.prompts.is_empty() {
        IconState::Attention
    } else if !state.mic_muted && active.clone().any(|r| r.kind.is_mic()) {
        IconState::MicLive
    } else if active.count() > 0 {
        IconState::Streaming
    } else {
        IconState::Idle
    }
}

fn describe(state: &EngineState) -> (Vec<Entry>, IconState, String) {
    let routes: Vec<&RouteView> = state
        .routes
        .iter()
        .filter(|r| r.status != RouteStatus::Stopped)
        .collect();
    let active = routes
        .iter()
        .filter(|r| r.status == RouteStatus::Active)
        .count();
    let mic_live = routes
        .iter()
        .any(|r| r.status == RouteStatus::Active && r.kind.is_mic());
    let icon = icon_state(state);

    let connected = |id: &str| {
        state
            .peers
            .iter()
            .any(|p| p.device_id == id && p.connection.is_connected())
    };
    let mut summary = match active {
        0 => {
            let n = state
                .peers
                .iter()
                .filter(|p| p.trusted && connected(&p.device_id))
                .count();
            match n {
                0 => "Not streaming".to_string(),
                1 => "1 device connected".to_string(),
                n => format!("{n} devices connected"),
            }
        }
        1 => routes
            .iter()
            .find(|r| r.status == RouteStatus::Active)
            .map(|r| format!("Streaming with {}", r.peer_name))
            .unwrap_or_default(),
        n => format!("{n} streams active"),
    };
    summary = match (icon, mic_live, state.mic_muted) {
        (IconState::Attention, ..) => format!("Waiting for your answer · {summary}"),
        (_, true, true) => format!("Microphone muted · {summary}"),
        (_, true, false) => format!("● Microphone live · {summary}"),
        _ => summary,
    };

    let mut entries = vec![Entry::Text(summary.clone()), Entry::Separator];
    for route in &routes {
        entries.push(Entry::Submenu {
            text: route_title(route),
            items: vec![
                Entry::Check {
                    id: format!("route-mute:{}", route.route_id),
                    text: "Mute".into(),
                    checked: route.muted,
                    enabled: true,
                },
                Entry::Action {
                    id: format!("route-stop:{}", route.route_id),
                    text: "Stop".into(),
                    enabled: true,
                },
            ],
        });
    }
    let mute_text = match &state.settings.desktop.mute_hotkey {
        Some(hotkey) => format!("Mute microphone ({})", shortcut_label(hotkey)),
        None => "Mute microphone".to_string(),
    };
    entries.push(Entry::Check {
        id: "mute".into(),
        text: mute_text,
        checked: state.mic_muted,
        enabled: mic_live || state.mic_muted,
    });
    entries.push(Entry::Action {
        id: "stop_all".into(),
        text: "Stop all streams".into(),
        enabled: !routes.is_empty(),
    });

    // Connected devices first, then the most recently seen.
    let mut recent: Vec<_> = state
        .peers
        .iter()
        .filter(|p| p.trusted && !p.blocked)
        .collect();
    recent.sort_by_key(|p| {
        (
            !connected(&p.device_id),
            std::cmp::Reverse(p.last_seen_unix),
        )
    });
    if !recent.is_empty() {
        let devices = recent
            .into_iter()
            .take(RECENT_DEVICES)
            .map(|p| {
                let mut items = Vec::new();
                if connected(&p.device_id) {
                    if p.can_play && state.capabilities.system_audio {
                        items.push(Entry::Action {
                            id: format!("listen:{}", p.device_id),
                            text: "Listen on this device".into(),
                            enabled: true,
                        });
                    }
                    if p.can_send_mic && state.capabilities.virtual_mic {
                        items.push(Entry::Action {
                            id: format!("microphone:{}", p.device_id),
                            text: "Use as microphone".into(),
                            enabled: true,
                        });
                    }
                    items.push(Entry::Action {
                        id: format!("disconnect:{}", p.device_id),
                        text: "Disconnect".into(),
                        enabled: true,
                    });
                } else {
                    items.push(Entry::Action {
                        id: format!("connect:{}", p.device_id),
                        text: "Connect".into(),
                        enabled: true,
                    });
                }
                let status = if connected(&p.device_id) {
                    "connected"
                } else {
                    "offline"
                };
                Entry::Submenu {
                    text: format!("{} ({status})", p.name),
                    items,
                }
            })
            .collect();
        entries.push(Entry::Separator);
        entries.push(Entry::Submenu {
            text: "Recent devices".into(),
            items: devices,
        });
    }
    entries.extend([
        Entry::Separator,
        Entry::Action {
            id: "open".into(),
            text: "Open SoundPush".into(),
            enabled: true,
        },
        Entry::Action {
            id: "quit".into(),
            text: "Quit SoundPush".into(),
            enabled: true,
        },
    ]);
    (entries, icon, format!("SoundPush — {summary}"))
}

/// The app icon with a status dot in the lower right corner: green streaming, red microphone
/// live, amber attention, red with a white bar for an error. A light ring keeps the dot visible
/// on dark and light taskbars.
fn badge(rgba: &[u8], width: u32, height: u32, state: IconState) -> Vec<u8> {
    let mut out = rgba.to_vec();
    let color: [u8; 3] = match state {
        IconState::Idle => return out,
        IconState::Streaming => [0x16, 0xA3, 0x4A],
        IconState::MicLive | IconState::Error => [0xDC, 0x26, 0x26],
        IconState::Attention => [0xF5, 0x9E, 0x0B],
    };
    let size = width.min(height) as f32;
    let radius = size * 0.26;
    let ring = (size * 0.06).max(1.0);
    let (cx, cy) = (width as f32 - radius - ring, height as f32 - radius - ring);
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let distance = (dx * dx + dy * dy).sqrt();
            let rgb = if distance <= radius {
                // An error dot carries a white bar so it differs from "microphone live".
                let bar = state == IconState::Error
                    && dx.abs() <= radius * 0.55
                    && dy.abs() <= radius * 0.18;
                if bar { [0xFF; 3] } else { color }
            } else if distance <= radius + ring {
                [0xFF; 3]
            } else {
                continue;
            };
            let i = ((y * width + x) * 4) as usize;
            if let Some(px) = out.get_mut(i..i + 4) {
                px[..3].copy_from_slice(&rgb);
                px[3] = 0xFF;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn badges_mark_the_lower_right_corner_only() {
        let base = vec![0u8; 32 * 32 * 4];
        assert_eq!(badge(&base, 32, 32, IconState::Idle), base);

        let streaming = badge(&base, 32, 32, IconState::Streaming);
        assert_eq!(pixel(&streaming, 32, 23, 23), [0x16, 0xA3, 0x4A, 0xFF]);
        assert_eq!(pixel(&streaming, 32, 2, 2), [0, 0, 0, 0]);

        let error = badge(&base, 32, 32, IconState::Error);
        let mic = badge(&base, 32, 32, IconState::MicLive);
        assert_ne!(error, mic, "error and microphone live must look different");
        // A malformed buffer never panics.
        let _ = badge(&[0u8; 7], 32, 32, IconState::Attention);
    }

    #[test]
    fn shortcuts_read_like_the_os() {
        let label = shortcut_label("CommandOrControl+Shift+M");
        assert!(label == "Ctrl+Shift+M" || label == "Cmd+Shift+M");
    }
}
