//! Length-prefixed framing for reliable stream transports (QUIC streams, TCP).
//!
//! Each frame is `u32 big-endian length` followed by `length` bytes.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::ProtocolError;

/// Maximum control frame size (64 KiB).
pub const MAX_CONTROL_FRAME: usize = 64 * 1024;
/// Maximum media frame size on stream transports (what receivers accept).
pub const MAX_MEDIA_FRAME: usize = crate::media::MEDIA_HEADER_LEN + crate::media::MAX_ACCEPTED_MEDIA_PAYLOAD;
/// Largest payload of a keep-alive or close frame.
pub const MAX_SMALL_FRAME: usize = 256;

/// Append a frame to `out`.
pub fn encode_frame(payload: &[u8], max: usize, out: &mut BytesMut) -> Result<(), ProtocolError> {
    if payload.len() > max {
        return Err(ProtocolError::FrameTooLarge {
            len: payload.len(),
            max,
        });
    }
    out.reserve(4 + payload.len());
    // Length fits in u32 because `max` is far below u32::MAX.
    out.put_u32(payload.len() as u32);
    out.put_slice(payload);
    Ok(())
}

/// Incremental frame decoder for a byte stream.
#[derive(Debug)]
pub struct FrameDecoder {
    buf: BytesMut,
    max: usize,
}

impl FrameDecoder {
    pub fn new(max: usize) -> Self {
        Self {
            buf: BytesMut::new(),
            max,
        }
    }

    /// Feed bytes read from the stream.
    pub fn extend(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Return the next complete frame, if any.
    pub fn next_frame(&mut self) -> Result<Option<Bytes>, ProtocolError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if len > self.max {
            return Err(ProtocolError::FrameTooLarge { len, max: self.max });
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        self.buf.advance(4);
        Ok(Some(self.buf.split_to(len).freeze()))
    }
}

/// Frame types multiplexed on one TLS-over-TCP stream (protocol 1.1).
///
/// Wire format: `kind u8 | length u32 big-endian | payload`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFrameKind {
    /// One `ControlMsg` (what a QUIC control stream frame carries).
    Control = 1,
    /// One media or probe datagram.
    Datagram = 2,
    /// Keep-alive: payload is the sender's `u64` microsecond clock.
    Ping = 3,
    /// Keep-alive answer echoing the ping payload.
    Pong = 4,
    /// Orderly close: `u32` application code followed by a short reason.
    Close = 5,
}

impl StreamFrameKind {
    fn from_u8(v: u8) -> Result<Self, ProtocolError> {
        Ok(match v {
            1 => Self::Control,
            2 => Self::Datagram,
            3 => Self::Ping,
            4 => Self::Pong,
            5 => Self::Close,
            other => return Err(ProtocolError::UnknownFrameKind(other)),
        })
    }

    /// Largest payload allowed for this kind.
    pub const fn max_len(self) -> usize {
        match self {
            Self::Control => MAX_CONTROL_FRAME,
            Self::Datagram => MAX_MEDIA_FRAME,
            Self::Ping | Self::Pong | Self::Close => MAX_SMALL_FRAME,
        }
    }
}

/// Append a typed stream frame to `out`.
pub fn encode_stream_frame(kind: StreamFrameKind, payload: &[u8], out: &mut BytesMut) -> Result<(), ProtocolError> {
    if payload.len() > kind.max_len() {
        return Err(ProtocolError::FrameTooLarge {
            len: payload.len(),
            max: kind.max_len(),
        });
    }
    out.reserve(5 + payload.len());
    out.put_u8(kind as u8);
    // Fits in u32: every kind's maximum is far below u32::MAX.
    out.put_u32(payload.len() as u32);
    out.put_slice(payload);
    Ok(())
}

/// Incremental decoder for typed stream frames. The buffer never holds more than one maximal
/// frame plus the last read, because oversized lengths are rejected before their bytes arrive.
#[derive(Debug, Default)]
pub struct StreamFrameDecoder {
    buf: BytesMut,
}

impl StreamFrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn extend(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Return the next complete frame, if any. An error means the stream is corrupt and must close.
    pub fn next_frame(&mut self) -> Result<Option<(StreamFrameKind, Bytes)>, ProtocolError> {
        if self.buf.len() < 5 {
            return Ok(None);
        }
        let kind = StreamFrameKind::from_u8(self.buf[0])?;
        let len = u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
        if len > kind.max_len() {
            return Err(ProtocolError::FrameTooLarge {
                len,
                max: kind.max_len(),
            });
        }
        if self.buf.len() < 5 + len {
            return Ok(None);
        }
        self.buf.advance(5);
        Ok(Some((kind, self.buf.split_to(len).freeze())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_frames_reassemble_and_enforce_limits() {
        let mut wire = BytesMut::new();
        encode_stream_frame(StreamFrameKind::Control, b"ctl", &mut wire).unwrap();
        encode_stream_frame(StreamFrameKind::Datagram, &[7u8; 900], &mut wire).unwrap();
        encode_stream_frame(StreamFrameKind::Ping, &5u64.to_be_bytes(), &mut wire).unwrap();
        assert!(
            encode_stream_frame(
                StreamFrameKind::Datagram,
                &[0u8; MAX_MEDIA_FRAME + 1],
                &mut BytesMut::new()
            )
            .is_err()
        );

        let mut dec = StreamFrameDecoder::new();
        let mut frames = Vec::new();
        for chunk in wire.chunks(13) {
            dec.extend(chunk);
            while let Some(f) = dec.next_frame().unwrap() {
                frames.push(f);
            }
        }
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0], (StreamFrameKind::Control, Bytes::from_static(b"ctl")));
        assert_eq!(frames[1].1.len(), 900);
        assert_eq!(frames[2].0, StreamFrameKind::Ping);

        let mut bad = StreamFrameDecoder::new();
        bad.extend(&[9, 0, 0, 0, 0]);
        assert_eq!(bad.next_frame(), Err(ProtocolError::UnknownFrameKind(9)));
        let mut huge = StreamFrameDecoder::new();
        huge.extend(&[2, 0, 1, 0, 0]);
        assert!(huge.next_frame().is_err());
    }

    #[test]
    fn split_reads_reassemble() {
        let mut wire = BytesMut::new();
        encode_frame(b"hello", MAX_CONTROL_FRAME, &mut wire).unwrap();
        encode_frame(b"world!", MAX_CONTROL_FRAME, &mut wire).unwrap();

        let mut dec = FrameDecoder::new(MAX_CONTROL_FRAME);
        let mut frames = Vec::new();
        for byte in wire.iter() {
            dec.extend(&[*byte]);
            while let Some(f) = dec.next_frame().unwrap() {
                frames.push(f);
            }
        }
        assert_eq!(
            frames,
            vec![Bytes::from_static(b"hello"), Bytes::from_static(b"world!")]
        );
    }

    #[test]
    fn oversized_frame_rejected() {
        let mut dec = FrameDecoder::new(8);
        dec.extend(&100u32.to_be_bytes());
        assert!(dec.next_frame().is_err());
    }
}
