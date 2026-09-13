//! Per-device permissions, enforced by the engine regardless of OS permissions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    Allow,
    Ask,
    Deny,
}

impl Policy {
    const fn to_bits(self) -> u32 {
        match self {
            Self::Allow => 0,
            Self::Ask => 1,
            Self::Deny => 2,
        }
    }

    const fn from_bits(bits: u32) -> Self {
        match bits & 0b11 {
            0 => Self::Allow,
            1 => Self::Ask,
            _ => Self::Deny,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    /// The peer may receive this device's system or app audio.
    ReceiveMyAudio,
    /// The peer may receive this device's microphone.
    UseMyMicrophone,
    /// The peer may send audio to be played or used here.
    SendAudioToMe,
    /// The peer may start/stop routes, change volume, or mute this device.
    ControlMe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    pub receive_my_audio: Policy,
    pub use_my_microphone: Policy,
    pub send_audio_to_me: Policy,
    pub control_me: Policy,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            receive_my_audio: Policy::Allow,
            use_my_microphone: Policy::Ask,
            send_audio_to_me: Policy::Allow,
            control_me: Policy::Allow,
        }
    }
}

impl Permissions {
    pub fn policy(&self, kind: PermissionKind) -> Policy {
        match kind {
            PermissionKind::ReceiveMyAudio => self.receive_my_audio,
            PermissionKind::UseMyMicrophone => self.use_my_microphone,
            PermissionKind::SendAudioToMe => self.send_audio_to_me,
            PermissionKind::ControlMe => self.control_me,
        }
    }

    pub fn set(&mut self, kind: PermissionKind, policy: Policy) {
        match kind {
            PermissionKind::ReceiveMyAudio => self.receive_my_audio = policy,
            PermissionKind::UseMyMicrophone => self.use_my_microphone = policy,
            PermissionKind::SendAudioToMe => self.send_audio_to_me = policy,
            PermissionKind::ControlMe => self.control_me = policy,
        }
    }

    /// Compact form for the `PermissionChanged` control message.
    pub fn to_bits(&self) -> u32 {
        self.receive_my_audio.to_bits()
            | (self.use_my_microphone.to_bits() << 2)
            | (self.send_audio_to_me.to_bits() << 4)
            | (self.control_me.to_bits() << 6)
    }

    pub fn from_bits(bits: u32) -> Self {
        Self {
            receive_my_audio: Policy::from_bits(bits),
            use_my_microphone: Policy::from_bits(bits >> 2),
            send_audio_to_me: Policy::from_bits(bits >> 4),
            control_me: Policy::from_bits(bits >> 6),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_ask_for_microphone() {
        let p = Permissions::default();
        assert_eq!(p.policy(PermissionKind::UseMyMicrophone), Policy::Ask);
        assert_eq!(p.policy(PermissionKind::ReceiveMyAudio), Policy::Allow);
    }

    #[test]
    fn bits_roundtrip() {
        let mut p = Permissions::default();
        p.set(PermissionKind::ControlMe, Policy::Deny);
        assert_eq!(Permissions::from_bits(p.to_bits()), p);
    }
}
