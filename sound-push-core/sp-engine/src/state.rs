//! Immutable state snapshot rendered by every UI.
//!
//! UIs contain no business logic: they display this snapshot and send commands.

use serde::{Deserialize, Serialize};
use sp_security::Permissions;

use crate::error::{ErrorView, Severity};
use crate::settings::Settings;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineState {
    /// Increments on every change.
    pub revision: u64,
    pub local: LocalDevice,
    pub peers: Vec<PeerView>,
    pub routes: Vec<RouteView>,
    pub pairing: PairingView,
    pub requests: Vec<RouteRequestPrompt>,
    pub notices: Vec<NoticeView>,
    pub settings: Settings,
    pub capabilities: LocalCapabilities,
    pub audio_devices: Vec<AudioDeviceView>,
    pub mic_level_db: f32,
    /// Running or last network self-test per device.
    pub network_tests: Vec<crate::nettest::NetworkTestView>,
    /// Microphone mute (tray, global shortcuts): silences every microphone route on this device.
    pub mic_muted: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalDevice {
    pub device_id: String,
    pub display_code: String,
    pub name: String,
    pub platform: String,
    pub port: u16,
    /// Loopback TLS-over-TCP port for USB via `adb reverse` (0 = not listening).
    pub tcp_port: u16,
    pub addresses: Vec<String>,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalCapabilities {
    pub system_audio: bool,
    pub app_audio: bool,
    pub microphone: bool,
    pub speaker: bool,
    pub virtual_mic: bool,
    /// Playback device the phone microphone is fed into (e.g. "CABLE Input (VB-Audio Virtual Cable)").
    pub virtual_mic_device: Option<String>,
    /// What other apps select as their microphone (e.g. "CABLE Output (VB-Audio Virtual Cable)").
    pub virtual_mic_input: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    PairingRequired,
    Connected,
    /// Connected, but loss or jitter stayed above the threshold (plan §19.1, `health.rs`).
    /// Routes keep running; everything that works while connected works here.
    Degraded,
    Reconnecting,
    WaitingForDevice,
    Incompatible,
}

impl ConnectionStatus {
    /// A session exists (`Connected` or `Degraded`).
    pub fn is_connected(self) -> bool {
        matches!(self, Self::Connected | Self::Degraded)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LinkQuality {
    #[default]
    Unknown,
    Excellent,
    Good,
    Poor,
}

impl LinkQuality {
    pub fn from_stats(rtt_ms: f64, loss_pct: f64, jitter_ms: f64) -> Self {
        if loss_pct > 3.0 || rtt_ms > 80.0 || jitter_ms > 30.0 {
            Self::Poor
        } else if loss_pct > 0.5 || rtt_ms > 20.0 || jitter_ms > 10.0 {
            Self::Good
        } else {
            Self::Excellent
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerView {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    pub trusted: bool,
    pub online: bool,
    pub connection: ConnectionStatus,
    pub quality: LinkQuality,
    pub rtt_ms: f64,
    pub addresses: Vec<String>,
    pub permissions: Option<Permissions>,
    pub auto_connect: bool,
    pub blocked: bool,
    pub last_seen_unix: u64,
    /// Endpoints the peer offers (e.g. can it capture system audio?).
    pub can_send_system_audio: bool,
    pub can_send_app_audio: bool,
    pub can_send_mic: bool,
    pub can_play: bool,
    pub has_virtual_mic: bool,
    /// "quic" or "tcp" (USB via adb) while connected, empty otherwise.
    pub transport: String,
}

/// User-level route type. Named from the local device's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteKind {
    /// This device's system audio plays on the peer.
    SendSystemAudio,
    /// This device's app audio (Android playback capture) plays on the peer.
    SendAppAudio,
    /// This device's microphone becomes the peer's virtual mic.
    SendMicToVirtualMic,
    /// This device's microphone plays on the peer's speakers.
    SendMicToSpeaker,
    /// The peer's system audio plays here.
    ReceiveSystemAudio,
    /// The peer's app audio plays here.
    ReceiveAppAudio,
    /// The peer's microphone becomes this device's virtual mic.
    ReceiveMicToVirtualMic,
    /// The peer's microphone plays on this device's speakers.
    ReceiveMicToSpeaker,
}

impl RouteKind {
    pub fn local_is_source(self) -> bool {
        matches!(
            self,
            Self::SendSystemAudio
                | Self::SendAppAudio
                | Self::SendMicToVirtualMic
                | Self::SendMicToSpeaker
        )
    }

    /// Wire endpoint ids (source, sink).
    pub fn endpoints(self) -> (&'static str, &'static str) {
        match self {
            Self::SendSystemAudio | Self::ReceiveSystemAudio => ("system", "speaker"),
            Self::SendAppAudio | Self::ReceiveAppAudio => ("apps", "speaker"),
            Self::SendMicToVirtualMic | Self::ReceiveMicToVirtualMic => ("mic", "virtual-mic"),
            Self::SendMicToSpeaker | Self::ReceiveMicToSpeaker => ("mic", "speaker"),
        }
    }

    /// The same route seen from the other device.
    pub fn mirrored(self) -> Self {
        match self {
            Self::SendSystemAudio => Self::ReceiveSystemAudio,
            Self::SendAppAudio => Self::ReceiveAppAudio,
            Self::SendMicToVirtualMic => Self::ReceiveMicToVirtualMic,
            Self::SendMicToSpeaker => Self::ReceiveMicToSpeaker,
            Self::ReceiveSystemAudio => Self::SendSystemAudio,
            Self::ReceiveAppAudio => Self::SendAppAudio,
            Self::ReceiveMicToVirtualMic => Self::SendMicToVirtualMic,
            Self::ReceiveMicToSpeaker => Self::SendMicToSpeaker,
        }
    }

    pub fn from_endpoints(source: &str, sink: &str, local_is_source: bool) -> Option<Self> {
        let send = match (source, sink) {
            ("system", "speaker") => Self::SendSystemAudio,
            ("apps", "speaker") => Self::SendAppAudio,
            ("mic", "virtual-mic") => Self::SendMicToVirtualMic,
            ("mic", "speaker") => Self::SendMicToSpeaker,
            _ => return None,
        };
        Some(if local_is_source {
            send
        } else {
            send.mirrored()
        })
    }

    pub fn is_mic(self) -> bool {
        matches!(
            self,
            Self::SendMicToVirtualMic
                | Self::SendMicToSpeaker
                | Self::ReceiveMicToVirtualMic
                | Self::ReceiveMicToSpeaker
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteStatus {
    Requesting,
    WaitingForApproval,
    Starting,
    Active,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RouteStats {
    pub codec: String,
    pub bitrate_kbps: u32,
    pub latency_ms: f64,
    pub buffer_ms: f64,
    pub jitter_ms: f64,
    pub loss_pct: f64,
    pub underruns: u64,
    pub drift_ppm: i32,
    pub level_db: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteView {
    pub route_id: String,
    pub peer_id: String,
    pub peer_name: String,
    pub kind: RouteKind,
    pub status: RouteStatus,
    pub started_unix: u64,
    pub elapsed_secs: u64,
    pub volume: f32,
    pub muted: bool,
    pub stats: RouteStats,
    pub keep_running: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    /// Present while "Pair a device" is open on this device.
    pub qr_uri: Option<String>,
    pub qr_expires_unix: u64,
    /// Code comparisons waiting for the user.
    pub prompts: Vec<PairingPrompt>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingPrompt {
    pub peer_id: String,
    pub peer_name: String,
    pub platform: String,
    pub code: String,
    /// True when the other side already confirmed.
    pub peer_confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRequestPrompt {
    pub request_id: u64,
    pub peer_id: String,
    pub peer_name: String,
    pub kind: RouteKind,
    pub expires_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeView {
    pub id: u64,
    pub key: String,
    pub args: Vec<String>,
    pub severity: Severity,
    pub error: Option<ErrorView>,
    pub created_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceView {
    pub id: String,
    pub name: String,
    pub is_input: bool,
    pub is_default: bool,
    /// Playback side of a virtual cable that can act as a microphone for other apps.
    pub virtual_cable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_kinds_mirror_and_map_endpoints() {
        for kind in [
            RouteKind::SendSystemAudio,
            RouteKind::SendAppAudio,
            RouteKind::SendMicToVirtualMic,
            RouteKind::SendMicToSpeaker,
        ] {
            let (src, sink) = kind.endpoints();
            assert_eq!(RouteKind::from_endpoints(src, sink, true), Some(kind));
            assert_eq!(
                RouteKind::from_endpoints(src, sink, false),
                Some(kind.mirrored())
            );
            assert_eq!(kind.mirrored().mirrored(), kind);
            assert!(kind.local_is_source());
            assert!(!kind.mirrored().local_is_source());
        }
        assert_eq!(RouteKind::from_endpoints("x", "y", true), None);
    }

    #[test]
    fn quality_thresholds() {
        assert_eq!(
            LinkQuality::from_stats(3.0, 0.0, 1.0),
            LinkQuality::Excellent
        );
        assert_eq!(LinkQuality::from_stats(30.0, 0.0, 1.0), LinkQuality::Good);
        assert_eq!(LinkQuality::from_stats(3.0, 5.0, 1.0), LinkQuality::Poor);
    }

    #[test]
    fn state_serializes_camel_case() {
        let json = serde_json::to_string(&EngineState::default()).unwrap();
        assert!(json.contains("\"audioDevices\""));
        assert!(json.contains("\"deviceName\""));
    }
}
