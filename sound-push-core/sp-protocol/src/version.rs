//! Protocol version negotiation and capability bits.

use std::fmt;

use crate::ProtocolError;

/// A protocol version (major.minor). Major changes are breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    pub const fn to_u32(self) -> u32 {
        ((self.major as u32) << 16) | self.minor as u32
    }

    pub const fn from_u32(v: u32) -> Self {
        Self {
            major: (v >> 16) as u16,
            minor: (v & 0xFFFF) as u16,
        }
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Inclusive range of supported protocol versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionRange {
    pub min: ProtocolVersion,
    pub max: ProtocolVersion,
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}–{}", self.min, self.max)
    }
}

/// The range this build supports. 1.1 adds session tickets, network tests, route
/// reconfiguration and the TCP transport; every 1.1 feature is also gated by a capability bit,
/// so 1.0 peers keep working.
pub const LOCAL_VERSIONS: VersionRange = VersionRange {
    min: ProtocolVersion::new(1, 0),
    max: ProtocolVersion::new(1, 1),
};

impl VersionRange {
    /// Highest version supported by both sides.
    pub fn negotiate(self, peer: VersionRange) -> Result<ProtocolVersion, ProtocolError> {
        let low = self.min.max(peer.min);
        let high = self.max.min(peer.max);
        if low <= high {
            Ok(high)
        } else {
            Err(ProtocolError::NoCommonVersion { local: self, peer })
        }
    }
}

/// Capability bits advertised in discovery and hello messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Capabilities(pub u64);

impl Capabilities {
    pub const SOURCE_SYSTEM_AUDIO: u64 = 1 << 0;
    pub const SOURCE_APP_AUDIO: u64 = 1 << 1;
    pub const SOURCE_MICROPHONE: u64 = 1 << 2;
    /// Can send system audio and the microphone mixed into one stream (plan §5.1, §9.2; the
    /// `mixed` endpoint). A requester offers that source only to peers advertising this bit, and
    /// a peer without it rejects the endpoint as unsupported, so 1.0 builds are unaffected.
    pub const SOURCE_MIXED: u64 = 1 << 3;
    pub const SINK_SPEAKER: u64 = 1 << 8;
    pub const SINK_VIRTUAL_MIC: u64 = 1 << 9;
    pub const CODEC_OPUS: u64 = 1 << 16;
    pub const CODEC_PCM: u64 = 1 << 17;
    pub const FEATURE_REDUNDANCY: u64 = 1 << 24;
    pub const FEATURE_REMOTE_CONTROL: u64 = 1 << 25;
    pub const FEATURE_MIC_MONITOR: u64 = 1 << 26;
    /// Understands `SessionTicket` and `Hello.resume_token` (protocol 1.1).
    pub const FEATURE_SESSION_RESUME: u64 = 1 << 27;
    /// Answers `NetTestStart` and echoes probe datagrams (protocol 1.1).
    pub const FEATURE_NETWORK_TEST: u64 = 1 << 28;
    /// Applies codec, frame and channel changes carried by `RouteUpdate` (protocol 1.1).
    pub const FEATURE_ROUTE_RECONFIGURE: u64 = 1 << 29;
    /// Accepts TLS-over-TCP connections on its port (USB via `adb reverse`, protocol 1.1).
    pub const TRANSPORT_TCP: u64 = 1 << 30;
    /// Handles header-only `DTX` media packets: silence until the route's next audio packet.
    /// Senders use DTX only towards receivers advertising this bit.
    pub const FEATURE_DTX: u64 = 1 << 31;
    /// Applies noise suppression to a microphone stream it receives when the source device sets
    /// `StreamProfile.denoise` ("noise suppression on the other device", plan §4.3/§15.7).
    /// Sources denoise locally instead towards peers without this bit.
    pub const FEATURE_RECEIVER_DENOISE: u64 = 1 << 32;

    pub const fn has(self, bit: u64) -> bool {
        self.0 & bit == bit
    }

    #[must_use]
    pub const fn with(self, bit: u64) -> Self {
        Self(self.0 | bit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(a: (u16, u16), b: (u16, u16)) -> VersionRange {
        VersionRange {
            min: ProtocolVersion::new(a.0, a.1),
            max: ProtocolVersion::new(b.0, b.1),
        }
    }

    #[test]
    fn picks_highest_common() {
        let local = range((1, 0), (1, 4));
        let peer = range((1, 2), (2, 0));
        assert_eq!(local.negotiate(peer).unwrap(), ProtocolVersion::new(1, 4));
    }

    #[test]
    fn disjoint_fails() {
        assert!(
            range((1, 0), (1, 1))
                .negotiate(range((2, 0), (2, 3)))
                .is_err()
        );
    }

    #[test]
    fn u32_roundtrip() {
        let v = ProtocolVersion::new(3, 17);
        assert_eq!(ProtocolVersion::from_u32(v.to_u32()), v);
    }
}
