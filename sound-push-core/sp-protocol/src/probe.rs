//! Network-test probe datagrams (protocol 1.1, `FEATURE_NETWORK_TEST`).
//!
//! Probes share the datagram channel with media. Their first byte is never a valid media
//! header version, so peers that do not run a test (including 1.0 peers) drop them silently.
//!
//! ```text
//!  0      1      2 .. 5      6 .. 9    10 .. 17         18 .. 25          26 ..
//! +------+------+-----------+---------+----------------+-----------------+---------+
//! | 0xA5 | kind | test_id   | seq     | sent_us (u64)  | echo_us (u64)   | padding |
//! +------+------+-----------+---------+----------------+-----------------+---------+
//! ```
//! All integers are big-endian. `kind` 1 = probe (initiator → responder), 2 = echo.
//! Echoes never carry padding, so a responder always sends fewer bytes than it receives.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::ProtocolError;
use crate::media::{MAX_MEDIA_PAYLOAD, MEDIA_HEADER_LEN};

/// First byte of every probe datagram.
pub const PROBE_MAGIC: u8 = 0xA5;
pub const PROBE_HEADER_LEN: usize = 26;
/// Probes are no larger than the largest media datagram, so they measure the same path.
pub const MAX_PROBE_LEN: usize = MEDIA_HEADER_LEN + MAX_MEDIA_PAYLOAD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    Probe = 1,
    Echo = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub kind: ProbeKind,
    pub test_id: u32,
    pub seq: u32,
    /// Initiator clock (microseconds) when the probe was sent.
    pub sent_us: u64,
    /// Responder clock when the probe arrived (0 in probes).
    pub echo_us: u64,
}

/// True when a datagram is a probe rather than media.
pub fn is_probe(datagram: &[u8]) -> bool {
    datagram.first() == Some(&PROBE_MAGIC)
}

impl Probe {
    /// Serialize, padded with zeros to `total_len` (clamped to the header and [`MAX_PROBE_LEN`]).
    pub fn encode(&self, total_len: usize) -> Bytes {
        let len = total_len.clamp(PROBE_HEADER_LEN, MAX_PROBE_LEN);
        let mut buf = BytesMut::with_capacity(len);
        buf.put_u8(PROBE_MAGIC);
        buf.put_u8(self.kind as u8);
        buf.put_u32(self.test_id);
        buf.put_u32(self.seq);
        buf.put_u64(self.sent_us);
        buf.put_u64(self.echo_us);
        buf.resize(len, 0);
        buf.freeze()
    }

    /// Parse an untrusted datagram. Never panics.
    pub fn decode(datagram: &[u8]) -> Result<Self, ProtocolError> {
        if datagram.len() < PROBE_HEADER_LEN {
            return Err(ProtocolError::TooShort {
                len: datagram.len(),
                need: PROBE_HEADER_LEN,
            });
        }
        if datagram.len() > MAX_PROBE_LEN {
            return Err(ProtocolError::PayloadTooLarge {
                len: datagram.len(),
                max: MAX_PROBE_LEN,
            });
        }
        let mut r = datagram;
        if r.get_u8() != PROBE_MAGIC {
            return Err(ProtocolError::Decode("not a probe".into()));
        }
        let kind = match r.get_u8() {
            1 => ProbeKind::Probe,
            2 => ProbeKind::Echo,
            other => return Err(ProtocolError::Decode(format!("probe kind {other}"))),
        };
        Ok(Self {
            kind,
            test_id: r.get_u32(),
            seq: r.get_u32(),
            sent_us: r.get_u64(),
            echo_us: r.get_u64(),
        })
    }

    /// The echo a responder sends back for this probe.
    pub fn echo(&self, received_us: u64) -> Self {
        Self {
            kind: ProbeKind::Echo,
            echo_us: received_us,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MediaPacket;
    use proptest::prelude::*;

    #[test]
    fn roundtrip_padding_and_echo() {
        let p = Probe {
            kind: ProbeKind::Probe,
            test_id: 3,
            seq: 99,
            sent_us: 123_456,
            echo_us: 0,
        };
        let wire = p.encode(1000);
        assert_eq!(wire.len(), 1000);
        assert!(is_probe(&wire));
        assert_eq!(Probe::decode(&wire).unwrap(), p);
        let echo = p.echo(777).encode(0);
        assert_eq!(echo.len(), PROBE_HEADER_LEN);
        let e = Probe::decode(&echo).unwrap();
        assert_eq!((e.kind, e.seq, e.echo_us), (ProbeKind::Echo, 99, 777));
        assert_eq!(p.encode(usize::MAX).len(), MAX_PROBE_LEN);
    }

    #[test]
    fn probes_are_never_media() {
        let wire = Probe {
            kind: ProbeKind::Probe,
            test_id: 1,
            seq: 1,
            sent_us: 1,
            echo_us: 0,
        }
        .encode(64);
        assert!(MediaPacket::decode(wire).is_err());
    }

    proptest! {
        #[test]
        fn decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..1500)) {
            let _ = Probe::decode(&data);
        }
    }
}
