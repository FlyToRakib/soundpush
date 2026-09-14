//! Global shortcuts (plan §13.2 "Hotkeys", §5.1): toggle the microphone mute, and push-to-talk
//! (the microphone is live only while the keys are held). Stored in `desktop.muteHotkey` and
//! `desktop.pushToTalkHotkey` as accelerators such as "Ctrl+Shift+M".
//!
//! Shortcuts are registered from the settings whenever they change. A shortcut another app
//! already owns cannot be registered; that error is kept and shown next to the setting.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tracing::{info, warn};

use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Mute,
    PushToTalk,
}

/// Why a shortcut is not active, as an i18n key (`hotkey.error.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyError {
    /// Not a valid key combination.
    Invalid,
    /// The other SoundPush shortcut uses the same keys.
    Duplicate,
    /// Another app already uses these keys, or the OS refused them.
    Unavailable,
}

impl HotkeyError {
    pub fn key(self) -> &'static str {
        match self {
            Self::Invalid => "error.hotkey.invalid",
            Self::Duplicate => "error.hotkey.duplicate",
            Self::Unavailable => "error.hotkey.unavailable",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub mute: Option<HotkeyError>,
    pub push_to_talk: Option<HotkeyError>,
}

#[derive(Default)]
struct Slot {
    accelerator: Option<String>,
    shortcut: Option<Shortcut>,
    error: Option<HotkeyError>,
}

#[derive(Default)]
pub struct Hotkeys {
    slots: Mutex<(Slot, Slot)>,
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(on_shortcut)
        .build()
}

fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    let (Some(state), Some(hotkeys)) = (app.try_state::<AppState>(), app.try_state::<Hotkeys>())
    else {
        return;
    };
    let Some(engine) = state.engine.get() else {
        return;
    };
    let Ok(slots) = hotkeys.slots.lock() else {
        return;
    };
    let is = |slot: &Slot| {
        slot.shortcut
            .as_ref()
            .is_some_and(|s| s.id() == shortcut.id())
    };
    let (mute, push_to_talk) = (is(&slots.0), is(&slots.1));
    drop(slots);
    // The OS does not repeat a held shortcut, so a toggle happens once per press.
    let result = match event.state() {
        ShortcutState::Pressed if mute => engine.set_mic_muted(!engine.state().mic_muted),
        ShortcutState::Pressed if push_to_talk => engine.set_mic_muted(false),
        ShortcutState::Released if push_to_talk => engine.set_mic_muted(true),
        _ => Ok(()),
    };
    if let Err(e) = result {
        warn!(error = %e, "shortcut could not change the microphone mute");
    }
}

/// Normalise and parse an accelerator ("ctrl + shift + m" → "Ctrl+Shift+M").
fn parse(accelerator: &str) -> Result<(String, Shortcut), HotkeyError> {
    let normal = accelerator
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("+");
    let shortcut: Shortcut = normal.parse().map_err(|_| HotkeyError::Invalid)?;
    Ok((normal, shortcut))
}

/// Check that `accelerator` can be used for `kind` right now, registering it on success.
/// The settings are saved by the caller afterwards; [`apply`] then sees nothing to change.
pub fn set(app: &AppHandle, kind: Kind, accelerator: Option<&str>) -> Result<(), HotkeyError> {
    let Some(hotkeys) = app.try_state::<Hotkeys>() else {
        return Err(HotkeyError::Unavailable);
    };
    let Ok(mut slots) = hotkeys.slots.lock() else {
        return Err(HotkeyError::Unavailable);
    };
    let (mute, push_to_talk) = &mut *slots;
    let (slot, other) = match kind {
        Kind::Mute => (mute, &*push_to_talk),
        Kind::PushToTalk => (push_to_talk, &*mute),
    };
    let parsed = accelerator.map(parse).transpose()?;
    if let Some((_, shortcut)) = &parsed
        && other
            .shortcut
            .as_ref()
            .is_some_and(|s| s.id() == shortcut.id())
    {
        return Err(HotkeyError::Duplicate);
    }
    replace(app, kind, slot, parsed)
}

/// Register the shortcuts saved in settings (at start-up and after every settings change).
pub fn apply(app: &AppHandle, mute: Option<&str>, push_to_talk: Option<&str>) {
    let Some(hotkeys) = app.try_state::<Hotkeys>() else {
        return;
    };
    let Ok(mut slots) = hotkeys.slots.lock() else {
        return;
    };
    for (kind, wanted) in [(Kind::Mute, mute), (Kind::PushToTalk, push_to_talk)] {
        let (mute_slot, push_to_talk_slot) = &mut *slots;
        let (slot, other) = match kind {
            Kind::Mute => (mute_slot, &*push_to_talk_slot),
            Kind::PushToTalk => (push_to_talk_slot, &*mute_slot),
        };
        let wanted = wanted.map(parse).transpose();
        let unchanged = match &wanted {
            Ok(w) => w.as_ref().map(|(a, _)| a.as_str()) == slot.accelerator.as_deref(),
            Err(_) => false,
        };
        if unchanged {
            continue;
        }
        let result = wanted.and_then(|parsed| {
            if let Some((_, shortcut)) = &parsed
                && other
                    .shortcut
                    .as_ref()
                    .is_some_and(|s| s.id() == shortcut.id())
            {
                return Err(HotkeyError::Duplicate);
            }
            replace(app, kind, slot, parsed)
        });
        if let Err(e) = result {
            warn!(?kind, error = ?e, "global shortcut not registered");
            slot.error = Some(e);
        }
    }
}

pub fn status(app: &AppHandle) -> HotkeyStatus {
    app.try_state::<Hotkeys>()
        .and_then(|h| {
            h.slots.lock().ok().map(|s| HotkeyStatus {
                mute: s.0.error,
                push_to_talk: s.1.error,
            })
        })
        .unwrap_or_default()
}

fn replace(
    app: &AppHandle,
    kind: Kind,
    slot: &mut Slot,
    parsed: Option<(String, Shortcut)>,
) -> Result<(), HotkeyError> {
    let manager = app.global_shortcut();
    if let Some((accelerator, shortcut)) = &parsed {
        if slot
            .shortcut
            .as_ref()
            .is_some_and(|s| s.id() == shortcut.id())
        {
            slot.accelerator = Some(accelerator.clone());
            slot.error = None;
            return Ok(());
        }
        manager.register(*shortcut).map_err(|e| {
            warn!(error = %e, accelerator, "shortcut unavailable");
            HotkeyError::Unavailable
        })?;
    }
    if let Some(old) = slot.shortcut.take() {
        let _ = manager.unregister(old);
    }
    let had_push_to_talk = kind == Kind::PushToTalk && slot.accelerator.is_some();
    slot.accelerator = parsed.as_ref().map(|(a, _)| a.clone());
    slot.shortcut = parsed.map(|(_, s)| s);
    slot.error = None;
    info!(?kind, accelerator = ?slot.accelerator, "global shortcut updated");

    // With push-to-talk the microphone starts muted; removing it gives the microphone back.
    if kind == Kind::PushToTalk
        && let Some(engine) = app
            .try_state::<AppState>()
            .and_then(|s| s.engine.get().cloned())
    {
        if slot.shortcut.is_some() {
            let _ = engine.set_mic_muted(true);
        } else if had_push_to_talk {
            let _ = engine.set_mic_muted(false);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_are_normalised_and_validated() {
        let (normal, _) = parse("Ctrl + Shift + M").unwrap_or_else(|_| panic!("valid accelerator"));
        assert_eq!(normal, "Ctrl+Shift+M");
        assert!(parse("F13").is_ok());
        assert_eq!(parse("Ctrl+Nope").err(), Some(HotkeyError::Invalid));
        assert_eq!(parse("").err(), Some(HotkeyError::Invalid));
    }
}
