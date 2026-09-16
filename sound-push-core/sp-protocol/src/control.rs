//! Control messages (protobuf wire format, defined with `prost` derives so no
//! `protoc` is needed at build time).
//!
//! Compatibility rules:
//! - Never reuse or renumber a field tag.
//! - New fields must be optional; unknown fields are ignored by older peers.
//! - New features are gated by [`crate::Capabilities`] bits, not version numbers.

use bytes::Bytes;
use prost::Message;

use crate::ProtocolError;
use crate::framing::MAX_CONTROL_FRAME;

/// Envelope for every control message.
#[derive(Clone, PartialEq, Message)]
pub struct ControlMsg {
    /// Request correlation id (0 for unsolicited messages).
    #[prost(uint32, tag = "1")]
    pub request_id: u32,
    #[prost(
        oneof = "control_msg::Body",
        tags = "10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29"
    )]
    pub body: Option<control_msg::Body>,
}

pub mod control_msg {
    use super::*;

    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "10")]
        Hello(Hello),
        #[prost(message, tag = "11")]
        RouteRequest(RouteRequest),
        #[prost(message, tag = "12")]
        RouteAccept(RouteAccept),
        #[prost(message, tag = "13")]
        RouteReject(RouteReject),
        #[prost(message, tag = "14")]
        RouteUpdate(RouteUpdate),
        #[prost(message, tag = "15")]
        RouteStop(RouteStop),
        #[prost(message, tag = "16")]
        VolumeSet(VolumeSet),
        #[prost(message, tag = "17")]
        MuteSet(MuteSet),
        #[prost(message, tag = "18")]
        StatsReport(StatsReport),
        #[prost(message, tag = "19")]
        Ping(Ping),
        #[prost(message, tag = "20")]
        Pong(Pong),
        #[prost(message, tag = "21")]
        PermissionChanged(PermissionChanged),
        #[prost(message, tag = "22")]
        Notice(Notice),
        #[prost(message, tag = "23")]
        Goodbye(Goodbye),
        #[prost(message, tag = "24")]
        PairRequest(PairRequest),
        #[prost(message, tag = "25")]
        PairResult(PairResult),
        // Protocol 1.1. Only sent to peers advertising the matching capability bit:
        // a 1.0 peer cannot decode these and would drop the session.
        #[prost(message, tag = "26")]
        SessionTicket(SessionTicket),
        #[prost(message, tag = "27")]
        NetTestStart(NetTestStart),
        #[prost(message, tag = "28")]
        NetTestReady(NetTestReady),
        #[prost(message, tag = "29")]
        NetTestStop(NetTestStop),
    }
}

/// A single-use resume token for the next connection to the issuing device
/// (`FEATURE_SESSION_RESUME`). Presented back in `Hello.resume_token`.
#[derive(Clone, PartialEq, Message)]
pub struct SessionTicket {
    /// Opaque 256-bit value.
    #[prost(bytes = "bytes", tag = "1")]
    pub token: Bytes,
    /// Seconds the issuer will accept the token for.
    #[prost(uint32, tag = "2")]
    pub lifetime_secs: u32,
}

/// Ask the peer to echo probe datagrams for a network test (`FEATURE_NETWORK_TEST`).
#[derive(Clone, PartialEq, Message)]
pub struct NetTestStart {
    #[prost(uint32, tag = "1")]
    pub test_id: u32,
    /// Upper bound of the test; the responder stops echoing afterwards.
    #[prost(uint32, tag = "2")]
    pub duration_ms: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct NetTestReady {
    #[prost(uint32, tag = "1")]
    pub test_id: u32,
    #[prost(bool, tag = "2")]
    pub accepted: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct NetTestStop {
    #[prost(uint32, tag = "1")]
    pub test_id: u32,
}

/// Sent by an untrusted peer after `Hello` to start pairing.
#[derive(Clone, PartialEq, Message)]
pub struct PairRequest {
    /// HMAC proof of the QR secret. Absent for code (SAS) pairing.
    #[prost(bytes = "bytes", optional, tag = "1")]
    pub qr_proof: Option<Bytes>,
}

/// Each side's pairing decision. Pairing completes when both accepted.
#[derive(Clone, PartialEq, Message)]
pub struct PairResult {
    #[prost(bool, tag = "1")]
    pub accepted: bool,
}

/// First message in both directions after the secure handshake.
#[derive(Clone, PartialEq, Message)]
pub struct Hello {
    #[prost(uint32, tag = "1")]
    pub protocol_min: u32,
    #[prost(uint32, tag = "2")]
    pub protocol_max: u32,
    #[prost(string, tag = "3")]
    pub app_version: String,
    #[prost(bytes = "bytes", tag = "4")]
    pub device_id: Bytes,
    #[prost(string, tag = "5")]
    pub device_name: String,
    #[prost(enumeration = "Platform", tag = "6")]
    pub platform: i32,
    #[prost(uint64, tag = "7")]
    pub capabilities: u64,
    /// Present when resuming a recent session.
    #[prost(bytes = "bytes", optional, tag = "8")]
    pub resume_token: Option<Bytes>,
    #[prost(message, repeated, tag = "9")]
    pub endpoints: Vec<EndpointInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
pub enum Platform {
    Unknown = 0,
    Windows = 1,
    Linux = 2,
    MacOs = 3,
    Android = 4,
    Ios = 5,
}

/// A source or sink a device can offer.
#[derive(Clone, PartialEq, Message)]
pub struct EndpointInfo {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(enumeration = "EndpointKind", tag = "3")]
    pub kind: i32,
    #[prost(bool, tag = "4")]
    pub is_default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
pub enum EndpointKind {
    Unspecified = 0,
    SourceSystemAudio = 1,
    SourceAppAudio = 2,
    SourceMicrophone = 3,
    /// System audio and the microphone mixed into one stream (`SOURCE_MIXED`). A peer that does
    /// not know the value decodes it as `Unspecified` and rejects the route.
    SourceMixed = 4,
    SinkSpeaker = 10,
    SinkVirtualMic = 11,
}

/// Audio stream parameters for a route.
#[derive(Clone, PartialEq, Message)]
pub struct StreamProfile {
    /// Codec id matching [`crate::Codec`].
    #[prost(uint32, tag = "1")]
    pub codec: u32,
    /// Opus bitrate in bits/s (ignored for PCM).
    #[prost(uint32, tag = "2")]
    pub bitrate: u32,
    #[prost(uint32, tag = "3")]
    pub channels: u32,
    /// Frame duration in microseconds (2500, 5000, 10000, 20000).
    #[prost(uint32, tag = "4")]
    pub frame_us: u32,
    #[prost(uint32, tag = "5")]
    pub jitter_min_ms: u32,
    #[prost(uint32, tag = "6")]
    pub jitter_max_ms: u32,
    #[prost(bool, tag = "7")]
    pub redundancy: bool,
    #[prost(bool, tag = "8")]
    pub adaptive_bitrate: bool,
}

/// Ask the peer to start a route. The requester may be either the source or the sink side.
#[derive(Clone, PartialEq, Message)]
pub struct RouteRequest {
    #[prost(uint32, tag = "1")]
    pub route: u32,
    /// Endpoint id on the device that produces audio.
    #[prost(string, tag = "2")]
    pub source_endpoint: String,
    /// Endpoint id on the device that plays audio.
    #[prost(string, tag = "3")]
    pub sink_endpoint: String,
    /// True when the requester is the audio source.
    #[prost(bool, tag = "4")]
    pub requester_is_source: bool,
    #[prost(message, optional, tag = "5")]
    pub profile: Option<StreamProfile>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RouteAccept {
    #[prost(uint32, tag = "1")]
    pub route: u32,
    #[prost(message, optional, tag = "2")]
    pub profile: Option<StreamProfile>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RouteReject {
    #[prost(uint32, tag = "1")]
    pub route: u32,
    #[prost(enumeration = "StopReason", tag = "2")]
    pub reason: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct RouteUpdate {
    #[prost(uint32, tag = "1")]
    pub route: u32,
    #[prost(message, optional, tag = "2")]
    pub profile: Option<StreamProfile>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RouteStop {
    #[prost(uint32, tag = "1")]
    pub route: u32,
    #[prost(enumeration = "StopReason", tag = "2")]
    pub reason: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
pub enum StopReason {
    Unspecified = 0,
    UserStopped = 1,
    PeerStopped = 2,
    PermissionDenied = 3,
    PermissionRevoked = 4,
    DeviceBusy = 5,
    AudioDeviceLost = 6,
    CallInterruption = 7,
    Superseded = 8,
    IncompatibleVersion = 9,
    NetworkLost = 10,
    UnsupportedEndpoint = 11,
    Timeout = 12,
    /// Too many pairing attempts from this address; try again in a minute. Peers that do not
    /// know the value treat it as an ordinary close.
    RateLimited = 13,
}

/// Target of a volume or mute command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
pub enum ControlTarget {
    /// The route's stream volume on the receiver.
    RouteStream = 0,
    /// The peer's physical speakers ("Mute PC").
    DeviceSpeakers = 1,
}

#[derive(Clone, PartialEq, Message)]
pub struct VolumeSet {
    #[prost(uint32, tag = "1")]
    pub route: u32,
    #[prost(enumeration = "ControlTarget", tag = "2")]
    pub target: i32,
    /// 0.0 – 2.0 (1.0 = unity).
    #[prost(float, tag = "3")]
    pub gain: f32,
}

#[derive(Clone, PartialEq, Message)]
pub struct MuteSet {
    #[prost(uint32, tag = "1")]
    pub route: u32,
    #[prost(enumeration = "ControlTarget", tag = "2")]
    pub target: i32,
    #[prost(bool, tag = "3")]
    pub muted: bool,
}

/// Receiver feedback to the sender (1 Hz).
#[derive(Clone, PartialEq, Message)]
pub struct StatsReport {
    #[prost(uint32, tag = "1")]
    pub route: u32,
    #[prost(uint32, tag = "2")]
    pub packets_received: u32,
    #[prost(uint32, tag = "3")]
    pub packets_lost: u32,
    #[prost(uint32, tag = "4")]
    pub jitter_us: u32,
    #[prost(uint32, tag = "5")]
    pub buffer_ms: u32,
    #[prost(uint32, tag = "6")]
    pub underruns: u32,
    #[prost(sint32, tag = "7")]
    pub drift_ppm: i32,
    #[prost(uint32, tag = "8")]
    pub output_latency_ms: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct Ping {
    #[prost(uint64, tag = "1")]
    pub nonce: u64,
    /// Sender monotonic time in microseconds.
    #[prost(uint64, tag = "2")]
    pub sent_us: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct Pong {
    #[prost(uint64, tag = "1")]
    pub nonce: u64,
    #[prost(uint64, tag = "2")]
    pub ping_sent_us: u64,
    #[prost(uint64, tag = "3")]
    pub received_us: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct PermissionChanged {
    #[prost(uint32, tag = "1")]
    pub permissions: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct Notice {
    /// Stable message key, localized by the receiving UI.
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(string, repeated, tag = "2")]
    pub args: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Goodbye {
    #[prost(enumeration = "StopReason", tag = "1")]
    pub reason: i32,
}

impl ControlMsg {
    pub fn new(request_id: u32, body: control_msg::Body) -> Self {
        Self {
            request_id,
            body: Some(body),
        }
    }

    pub fn to_bytes(&self) -> Bytes {
        Bytes::from(self.encode_to_vec())
    }

    /// Decode an untrusted control frame.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProtocolError> {
        if data.len() > MAX_CONTROL_FRAME {
            return Err(ProtocolError::FrameTooLarge {
                len: data.len(),
                max: MAX_CONTROL_FRAME,
            });
        }
        let msg = Self::decode(data).map_err(|e| ProtocolError::Decode(e.to_string()))?;
        if msg.body.is_none() {
            // A well-formed envelope whose body tag this build does not know comes from a newer
            // peer; stream readers skip it instead of treating it as corruption.
            return Err(if data.is_empty() {
                ProtocolError::Decode("empty body".into())
            } else {
                ProtocolError::UnknownMessage
            });
        }
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::control_msg::Body;
    use super::*;

    #[test]
    fn hello_roundtrip() {
        let msg = ControlMsg::new(
            1,
            Body::Hello(Hello {
                protocol_min: 0x0001_0000,
                protocol_max: 0x0001_0000,
                app_version: "0.1.0".into(),
                device_id: Bytes::from_static(&[7; 16]),
                device_name: "Desk".into(),
                platform: Platform::Windows as i32,
                capabilities: 0b1011,
                resume_token: None,
                endpoints: vec![EndpointInfo {
                    id: "system".into(),
                    name: "System audio".into(),
                    kind: EndpointKind::SourceSystemAudio as i32,
                    is_default: true,
                }],
            }),
        );
        let decoded = ControlMsg::from_bytes(&msg.to_bytes()).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn garbage_is_error_not_panic() {
        assert!(ControlMsg::from_bytes(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]).is_err());
        assert!(ControlMsg::from_bytes(&[]).is_err());
    }

    #[test]
    fn unknown_body_from_newer_peer_is_distinguishable() {
        // request_id = 1, then field 99 (a future body) as a length-delimited empty message.
        let future = [0x08, 0x01, 0x9A, 0x06, 0x00];
        assert_eq!(
            ControlMsg::from_bytes(&future),
            Err(ProtocolError::UnknownMessage)
        );
    }

    #[test]
    fn protocol_1_1_messages_roundtrip() {
        for body in [
            Body::SessionTicket(SessionTicket {
                token: Bytes::from_static(&[9; 32]),
                lifetime_secs: 600,
            }),
            Body::NetTestStart(NetTestStart {
                test_id: 7,
                duration_ms: 8000,
            }),
            Body::NetTestReady(NetTestReady {
                test_id: 7,
                accepted: true,
            }),
            Body::NetTestStop(NetTestStop { test_id: 7 }),
        ] {
            let msg = ControlMsg::new(0, body);
            assert_eq!(ControlMsg::from_bytes(&msg.to_bytes()).unwrap(), msg);
        }
    }
}
