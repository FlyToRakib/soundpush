//! Media datagram format.
//!
//! ```text
//!  0      1      2      3      4 .. 11                 12 .. 15
//! +------+------+------+------+------------------------+-----------+
//! | ver  | route| codec| flags| sample_timestamp (u64) | seq (u32) |
//! +------+------+------+------+------------------------+-----------+
//! | payload ...                                                    |
//! ```
//! All integers are big-endian.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::ProtocolError;

/// Current media header version.
pub const MEDIA_HEADER_VERSION: u8 = 1;
/// Size of the fixed media header in bytes.
pub const MEDIA_HEADER_LEN: usize = 16;
/// Largest payload accepted. Keeps datagrams under a 1280-byte IPv6 minimum MTU
/// after QUIC overhead.
pub const MAX_MEDIA_PAYLOAD: usize = 1200;

/// Audio codec carried in a media packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Codec {
    /// Interleaved signed 16-bit little-endian PCM.
    PcmS16Le = 1,
    /// Opus packet.
    Opus = 2,
}

impl TryFrom<u8> for Codec {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::PcmS16Le),
            2 => Ok(Self::Opus),
            other => Err(ProtocolError::UnknownCodec(other)),
        }
    }
}

/// Bit flags in the media header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MediaFlags(u8);

impl MediaFlags {
    /// Sender is in discontinuous transmission (silence); payload may be empty.
    pub const DTX: u8 = 0b0000_0001;
    /// Payload contains a redundant copy of the previous frame after the primary frame.
    pub const REDUNDANT: u8 = 0b0000_0010;
    /// Timeline discontinuity (source changed, device reopened). Receiver should resync.
    pub const DISCONTINUITY: u8 = 0b0000_0100;
    /// First packet of a stream.
    pub const MARKER: u8 = 0b0000_1000;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, flag: u8) -> bool {
        self.0 & flag == flag
    }

    #[must_use]
    pub const fn with(self, flag: u8) -> Self {
        Self(self.0 | flag)
    }
}

/// Decoded media header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaHeader {
    /// Route identifier within the session (0–255).
    pub route: u8,
    pub codec: Codec,
    pub flags: MediaFlags,
    /// Sender sample clock position of the first sample in this packet (48 kHz timeline).
    pub sample_timestamp: u64,
    /// Monotonic packet sequence number per route (wraps).
    pub seq: u32,
}

/// A media header plus its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPacket {
    pub header: MediaHeader,
    pub payload: Bytes,
}

impl MediaPacket {
    /// Serialize into a single datagram.
    pub fn encode(&self) -> Result<Bytes, ProtocolError> {
        if self.payload.len() > MAX_MEDIA_PAYLOAD {
            return Err(ProtocolError::PayloadTooLarge {
                len: self.payload.len(),
                max: MAX_MEDIA_PAYLOAD,
            });
        }
        let mut buf = BytesMut::with_capacity(MEDIA_HEADER_LEN + self.payload.len());
        buf.put_u8(MEDIA_HEADER_VERSION);
        buf.put_u8(self.header.route);
        buf.put_u8(self.header.codec as u8);
        buf.put_u8(self.header.flags.bits());
        buf.put_u64(self.header.sample_timestamp);
        buf.put_u32(self.header.seq);
        buf.put_slice(&self.payload);
        Ok(buf.freeze())
    }

    /// Parse an untrusted datagram. Never panics.
    pub fn decode(mut datagram: Bytes) -> Result<Self, ProtocolError> {
        if datagram.len() < MEDIA_HEADER_LEN {
            return Err(ProtocolError::TooShort {
                len: datagram.len(),
                need: MEDIA_HEADER_LEN,
            });
        }
        let payload_len = datagram.len() - MEDIA_HEADER_LEN;
        if payload_len > MAX_MEDIA_PAYLOAD {
            return Err(ProtocolError::PayloadTooLarge {
                len: payload_len,
                max: MAX_MEDIA_PAYLOAD,
            });
        }
        let version = datagram.get_u8();
        if version != MEDIA_HEADER_VERSION {
            return Err(ProtocolError::UnsupportedVersion(version));
        }
        let route = datagram.get_u8();
        let codec = Codec::try_from(datagram.get_u8())?;
        let flags = MediaFlags::from_bits(datagram.get_u8());
        let sample_timestamp = datagram.get_u64();
        let seq = datagram.get_u32();
        Ok(Self {
            header: MediaHeader {
                route,
                codec,
                flags,
                sample_timestamp,
                seq,
            },
            payload: datagram,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sample() -> MediaPacket {
        MediaPacket {
            header: MediaHeader {
                route: 3,
                codec: Codec::Opus,
                flags: MediaFlags::empty().with(MediaFlags::MARKER),
                sample_timestamp: 48_000 * 3600,
                seq: 42,
            },
            payload: Bytes::from_static(&[1, 2, 3, 4]),
        }
    }

    #[test]
    fn roundtrip() {
        let pkt = sample();
        let wire = pkt.encode().unwrap();
        assert_eq!(wire.len(), MEDIA_HEADER_LEN + 4);
        assert_eq!(MediaPacket::decode(wire).unwrap(), pkt);
    }

    #[test]
    fn rejects_short() {
        assert!(matches!(
            MediaPacket::decode(Bytes::from_static(&[1, 2, 3])),
            Err(ProtocolError::TooShort { .. })
        ));
    }

    #[test]
    fn rejects_unknown_codec_and_version() {
        let mut wire = sample().encode().unwrap().to_vec();
        wire[2] = 99;
        assert_eq!(
            MediaPacket::decode(Bytes::from(wire.clone())),
            Err(ProtocolError::UnknownCodec(99))
        );
        wire[0] = 7;
        assert_eq!(
            MediaPacket::decode(Bytes::from(wire)),
            Err(ProtocolError::UnsupportedVersion(7))
        );
    }

    #[test]
    fn rejects_oversized_payload() {
        let mut pkt = sample();
        pkt.payload = Bytes::from(vec![0u8; MAX_MEDIA_PAYLOAD + 1]);
        assert!(pkt.encode().is_err());
    }

    proptest! {
        #[test]
        fn decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..1500)) {
            let _ = MediaPacket::decode(Bytes::from(data));
        }
    }
}
