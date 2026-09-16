//! Secure transport for SoundPush.
//!
//! QUIC (quinn + rustls, TLS 1.3 only) carries:
//! - a reliable, length-framed **control stream** (first bidirectional stream), and
//! - unreliable **datagrams** for media.
//!
//! TLS 1.3 over TCP ([`TcpEndpoint`]) carries the same two channels as typed frames on one
//! stream, for links that forward TCP only (USB via `adb reverse`). Both produce a
//! [`SecureConnection`] with the same API.
//!
//! Both sides present self-signed certificates bound to their Ed25519 identity.
//! TLS verifies handshake signatures; *authorization* is done by the engine
//! against its trust store using [`SecureConnection::peer_public_key`]. When the
//! caller knows who it is dialing, the expected fingerprint is pinned during the
//! handshake itself.
//!
//! No `unsafe` except the Windows qWAVE calls that DSCP-mark media flows (`qos.rs`).
#![deny(unsafe_code)]

mod connection;
mod endpoint;
mod qos;
mod tcp;
mod tls;
mod verifier;

pub use connection::{ControlReceiver, ControlSender, PathStats, SecureConnection, TransportKind};
pub use endpoint::{DEFAULT_PORT, Endpoint, EndpointConfig, Handshake};
pub use qos::DSCP_EF;
pub use tcp::{ALPN_TCP, STALE_AFTER, TcpEndpoint, TcpHandshake};

/// ALPN protocol identifier.
pub const ALPN: &[u8] = b"soundpush/1";

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("failed to bind socket: {0}")]
    Bind(std::io::Error),
    #[error("tls configuration error: {0}")]
    Tls(String),
    #[error("connect error: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("connection error: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("peer certificate missing or invalid")]
    PeerIdentity,
    /// The address dialled is this device: the certificate on the other end was our own.
    #[error("that address is this device")]
    DialedSelf,
    #[error("stream write error: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("stream read error: {0}")]
    Read(#[from] quinn::ReadError),
    #[error("datagram error: {0}")]
    Datagram(#[from] quinn::SendDatagramError),
    #[error("protocol error: {0}")]
    Protocol(#[from] sp_protocol::ProtocolError),
    #[error("control stream closed")]
    Closed,
    #[error("timed out")]
    Timeout,
    #[error("security error: {0}")]
    Security(#[from] sp_security::SecurityError),
}

impl TransportError {
    /// The other device refused this one's certificate during the handshake: it does not
    /// recognise this device's key.
    ///
    /// Only this is evidence that a device needs pairing again. A peer that closes a handshake on
    /// purpose — two devices dialling each other at once, the duplicate dropped — or one that went
    /// away mid-handshake is not, and must not be counted as a refusal.
    pub fn is_certificate_refusal(&self) -> bool {
        match self {
            // TLS alerts travel in QUIC as crypto error codes, 0x100 + the alert.
            Self::Connection(quinn::ConnectionError::ConnectionClosed(close)) => {
                (0x100..0x200).contains(&u64::from(close.error_code))
            }
            Self::Io(e) => e
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<rustls::Error>())
                .is_some_and(|tls| matches!(tls, rustls::Error::AlertReceived(_))),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    fn closed(code: quinn::TransportErrorCode) -> TransportError {
        TransportError::Connection(quinn::ConnectionError::ConnectionClosed(
            quinn::ConnectionClose {
                error_code: code,
                frame_type: None,
                reason: Bytes::new(),
            },
        ))
    }

    #[test]
    fn only_a_refused_certificate_counts_as_a_refusal() {
        // bad_certificate (42), as a device that does not recognise our key sends it.
        assert!(closed(quinn::TransportErrorCode::crypto(42)).is_certificate_refusal());
        // Two devices dialled each other and one closed the duplicate during its handshake.
        assert!(!closed(quinn::TransportErrorCode::APPLICATION_ERROR).is_certificate_refusal());
        assert!(!TransportError::Timeout.is_certificate_refusal());
        assert!(!TransportError::Closed.is_certificate_refusal());
    }
}
