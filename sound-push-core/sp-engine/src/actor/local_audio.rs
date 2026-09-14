//! This device's own audio: following the system default device when it changes, the
//! microphone mute, per-app capture, and starting the phone microphone when another app opens
//! the virtual microphone.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use sp_audio_io::CaptureSource;
use sp_protocol::Capabilities;
use sp_protocol::control::StopReason;
use sp_security::DeviceId;
use tokio::sync::oneshot;
use tracing::{info, warn};

use super::{Actor, Route, route_key};
use crate::error::Severity;
use crate::settings::Settings;
use crate::state::{RouteKind, RouteStatus};

/// How often the OS is asked whether an app records from the virtual microphone.
const IN_USE_POLL: Duration = Duration::from_secs(2);
/// An automatically started microphone route stops after the virtual microphone is unused this long.
const AUTO_MIC_IDLE: Duration = Duration::from_secs(15);
/// A route whose audio fails again this soon after being reopened is not reopened again.
const REOPEN_BACKOFF: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct LocalAudio {
    last_poll: Option<Instant>,
    /// An app has the virtual microphone open and SoundPush already reacted to it.
    handled_use: bool,
    /// Route started because an app opened the virtual microphone.
    auto_route: Option<String>,
    idle_since: Option<Instant>,
    /// When each route's audio was last reopened on a new device.
    reopened: HashMap<String, Instant>,
}

/// Whether a route's audio on this device uses the system default device of a changed direction.
/// Routes pinned to a named device, per-app capture and the virtual microphone never follow it.
pub(super) fn follows_default(
    kind: RouteKind,
    settings: &Settings,
    input_changed: bool,
    output_changed: bool,
) -> bool {
    let (source, sink) = kind.endpoints();
    if kind.local_is_source() {
        match source {
            "system" => {
                output_changed
                    && settings.capture.system_device.is_none()
                    && settings.capture.app.is_none()
            }
            "mic" => input_changed && settings.mic.device.is_none(),
            _ => false,
        }
    } else {
        sink == "speaker" && output_changed && settings.output.device.is_none()
    }
}

impl Actor {
    /// What "send this computer's audio" captures: everything, or one app (or all but one).
    pub(super) fn system_audio_source(&self) -> CaptureSource {
        match &self.settings.capture.app {
            Some(process) => CaptureSource::Application {
                process: process.clone(),
                exclude: self.settings.capture.exclude_app,
            },
            None => CaptureSource::SystemLoopback(self.settings.capture.system_device.clone()),
        }
    }

    pub(super) fn set_mic_muted(&mut self, muted: bool) {
        self.mic_muted = muted;
        // Shared microphone encoders (every route using this computer's microphone).
        self.update_mic_groups();
        for r in self.routes.iter().filter(|r| r.kind.is_mic()) {
            // Both directions: this computer's microphone, and the phone's microphone feeding
            // the virtual microphone (what a push-to-talk shortcut is for).
            if let Some(c) = &r.sender_controls {
                c.muted.store(muted || r.muted, Ordering::Relaxed);
            }
            if let Some(c) = &r.receiver_controls {
                c.muted.store(muted || r.muted, Ordering::Relaxed);
            }
        }
    }

    /// The OS reported a device change: refresh the list and move running routes that use the
    /// system default device of a changed direction to the new default.
    pub(super) fn on_audio_devices_changed(&mut self, input_changed: bool, output_changed: bool) {
        self.refresh_audio_devices();
        if !input_changed && !output_changed {
            return;
        }
        let keys: Vec<String> = self
            .routes
            .iter()
            .filter(|r| {
                r.status == RouteStatus::Active
                    && follows_default(r.kind, &self.settings, input_changed, output_changed)
            })
            .map(Route::key)
            .collect();
        for key in keys {
            self.reopen_route_audio(&key);
        }
        let monitor_follows = (input_changed && self.settings.mic.device.is_none())
            || (output_changed && self.settings.output.device.is_none());
        if self.monitor.is_some() && monitor_follows {
            self.set_monitor(true);
        }
    }

    /// A route's audio device failed (for example the default headset was unplugged). A route on
    /// the system default reopens once on whatever is the default now; returns true if it did.
    /// Routes pinned to a named device are left to report the lost device.
    pub(super) fn reopen_on_default_device(&mut self, peer: DeviceId, route: u8) -> bool {
        let key = route_key(&peer, route);
        let Some(r) = self.routes.iter().find(|r| r.key() == key) else {
            return false;
        };
        if r.status != RouteStatus::Active || !follows_default(r.kind, &self.settings, true, true) {
            return false;
        }
        let now = Instant::now();
        self.local_audio
            .reopened
            .retain(|_, t| now.duration_since(*t) < REOPEN_BACKOFF);
        if self.local_audio.reopened.contains_key(&key) {
            return false;
        }
        self.local_audio.reopened.insert(key.clone(), now);
        self.reopen_route_audio(&key)
    }

    /// Restart a route's audio pipeline on the current devices. On failure the route stops with
    /// a notice. Returns whether the route is still running (the device reopens off the actor;
    /// a failure there stops the route through `fail_route`).
    fn reopen_route_audio(&mut self, key: &str) -> bool {
        let Some(pos) = self.routes.iter().position(|r| r.key() == key) else {
            return false;
        };
        let mut route = self.routes.remove(pos);
        // A capture shared with other routes must really reopen, not be joined again.
        if let Some(encoder) = route.encoder.clone() {
            self.close_encoder(&encoder);
        }
        self.release_pipelines(&mut route);
        let result = self.begin_pipelines(&mut route);
        let peer = route.peer;
        self.routes.insert(pos.min(self.routes.len()), route);
        match result {
            Ok(_) => {
                info!(route = key, "audio moved to the current default device");
                true
            }
            Err(e) => {
                warn!(route = key, error = %e, "could not reopen audio on the new device");
                let name = self.peer_name(&peer);
                self.error_notice(&e, vec![name]);
                self.stop_route_by_key(key, StopReason::AudioDeviceLost, true);
                false
            }
        }
    }

    /// Called every tick: remembers the phone used as microphone, and (when enabled) starts it
    /// while another app has the virtual microphone open, stopping it again afterwards.
    pub(super) fn check_virtual_mic_use(&mut self) {
        self.remember_mic_peer();
        if !self.settings.desktop.auto_start_mic {
            self.local_audio.auto_route = None;
            self.local_audio.handled_use = false;
            return;
        }
        let now = Instant::now();
        if self
            .local_audio
            .last_poll
            .is_some_and(|t| now.duration_since(t) < IN_USE_POLL)
        {
            return;
        }
        self.local_audio.last_poll = Some(now);

        let mic_route = self
            .routes
            .iter()
            .find(|r| {
                r.kind == RouteKind::ReceiveMicToVirtualMic && r.status != RouteStatus::Stopped
            })
            .map(Route::key);
        // The automatic route ended some other way (stopped by the user, device left).
        if self.local_audio.auto_route.is_some() && self.local_audio.auto_route != mic_route {
            self.local_audio.auto_route = None;
        }

        if self.hooks.virtual_mic_in_use() {
            self.local_audio.idle_since = None;
            // React once per use: if the user stops the route, it stays stopped.
            if !self.local_audio.handled_use {
                self.local_audio.handled_use = mic_route.is_some() || self.auto_start_mic();
            }
        } else {
            self.local_audio.handled_use = false;
            if let Some(key) = self.local_audio.auto_route.clone() {
                let since = *self.local_audio.idle_since.get_or_insert(now);
                if now.duration_since(since) >= AUTO_MIC_IDLE {
                    info!("virtual microphone no longer used; stopping the phone microphone");
                    self.local_audio.auto_route = None;
                    self.local_audio.idle_since = None;
                    self.stop_route_by_key(&key, StopReason::UserStopped, true);
                }
            }
        }
    }

    /// Start the phone microphone for an app that opened the virtual microphone: the device used
    /// last time, or the only connected device that has a microphone. Returns false when there is
    /// no such device yet (checked again on the next poll).
    fn auto_start_mic(&mut self) -> bool {
        if !self.local_capabilities().virtual_mic {
            return false;
        }
        let candidates: Vec<DeviceId> = self
            .sessions
            .iter()
            .filter(|(id, s)| {
                self.trust.get(id).is_some_and(|d| !d.blocked)
                    && Capabilities(s.hello.capabilities).has(Capabilities::SOURCE_MICROPHONE)
            })
            .map(|(id, _)| *id)
            .collect();
        let last = self
            .settings
            .desktop
            .last_mic_peer
            .as_deref()
            .and_then(DeviceId::from_hex);
        let peer = match last.filter(|id| candidates.contains(id)) {
            Some(id) => id,
            None if candidates.len() == 1 => match candidates.first() {
                Some(id) => *id,
                None => return false,
            },
            None => return false,
        };
        info!("an app opened the virtual microphone; starting the phone microphone");
        let (reply, _) = oneshot::channel();
        self.start_route(peer, RouteKind::ReceiveMicToVirtualMic, reply);
        self.local_audio.auto_route = self
            .routes
            .iter()
            .rev()
            .find(|r| r.peer == peer && r.kind == RouteKind::ReceiveMicToVirtualMic)
            .map(Route::key);
        let name = self.peer_name(&peer);
        self.notice("notice.autoMicStarted", vec![name], Severity::Info, None);
        true
    }

    fn remember_mic_peer(&mut self) {
        let active = self
            .routes
            .iter()
            .find(|r| {
                r.kind == RouteKind::ReceiveMicToVirtualMic && r.status == RouteStatus::Active
            })
            .map(|r| r.peer.to_hex());
        if let Some(hex) = active
            && self.settings.desktop.last_mic_peer.as_deref() != Some(hex.as_str())
        {
            self.settings.desktop.last_mic_peer = Some(hex);
            self.save_settings();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_routes_on_the_default_device_follow_it() {
        let mut s = Settings::default();
        assert!(follows_default(RouteKind::SendSystemAudio, &s, false, true));
        assert!(!follows_default(
            RouteKind::SendSystemAudio,
            &s,
            true,
            false
        ));
        assert!(follows_default(
            RouteKind::SendMicToVirtualMic,
            &s,
            true,
            false
        ));
        assert!(follows_default(
            RouteKind::ReceiveSystemAudio,
            &s,
            false,
            true
        ));
        // The virtual microphone is a named cable, never the default output.
        assert!(!follows_default(
            RouteKind::ReceiveMicToVirtualMic,
            &s,
            true,
            true
        ));

        s.output.device = Some("Headphones".into());
        s.capture.app = Some("spotify.exe".into());
        s.mic.device = Some("USB Mic".into());
        assert!(!follows_default(
            RouteKind::ReceiveSystemAudio,
            &s,
            true,
            true
        ));
        assert!(!follows_default(RouteKind::SendSystemAudio, &s, true, true));
        assert!(!follows_default(
            RouteKind::SendMicToSpeaker,
            &s,
            true,
            true
        ));
    }
}
