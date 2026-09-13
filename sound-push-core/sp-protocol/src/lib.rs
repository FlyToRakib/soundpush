//! SoundPush wire protocol.
//!
//! Pure data definitions and codecs with no I/O:
//! - [`media`]: the fixed 16-byte media datagram header.
//! - [`control`]: protobuf control messages exchanged on the reliable channel.
//! - [`framing`]: length-prefixed framing for stream transports.
//! - [`version`]: protocol version range negotiation and capability bits.
#![forbid(unsafe_code)]

pub mod control;
pub mod framing;
pub mod media;
pub mod version;

pub use media::{Codec, MediaFlags, MediaHeader, MediaPacket};
pub use version::{Capabilities, ProtocolVersion, VersionRange};

/// Errors produced while decoding untrusted protocol input.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("packet too short: {len} bytes, need at least {need}")]
    TooShort { len: usize, need: usize },
    #[error("unsupported media header version {0}")]
    UnsupportedVersion(u8),
    #[error("unknown codec id {0}")]
    UnknownCodec(u8),
    #[error("payload too large: {len} bytes, maximum {max}")]
    PayloadTooLarge { len: usize, max: usize },
    #[error("frame too large: {len} bytes, maximum {max}")]
    FrameTooLarge { len: usize, max: usize },
    #[error("malformed control message: {0}")]
    Decode(String),
    #[error("no common protocol version (local {local}, peer {peer})")]
    NoCommonVersion { local: VersionRange, peer: VersionRange },
}
