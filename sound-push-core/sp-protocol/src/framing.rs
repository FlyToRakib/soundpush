//! Length-prefixed framing for reliable stream transports (QUIC streams, TCP).
//!
//! Each frame is `u32 big-endian length` followed by `length` bytes.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::ProtocolError;

/// Maximum control frame size (64 KiB).
pub const MAX_CONTROL_FRAME: usize = 64 * 1024;
/// Maximum media frame size on stream transports.
pub const MAX_MEDIA_FRAME: usize = crate::media::MEDIA_HEADER_LEN + crate::media::MAX_MEDIA_PAYLOAD;

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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(frames, vec![Bytes::from_static(b"hello"), Bytes::from_static(b"world!")]);
    }

    #[test]
    fn oversized_frame_rejected() {
        let mut dec = FrameDecoder::new(8);
        dec.extend(&100u32.to_be_bytes());
        assert!(dec.next_frame().is_err());
    }
}
