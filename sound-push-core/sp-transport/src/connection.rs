//! An authenticated QUIC connection to a peer.

use std::net::SocketAddr;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use rustls::pki_types::CertificateDer;
use sp_protocol::control::ControlMsg;
use sp_protocol::framing::{FrameDecoder, MAX_CONTROL_FRAME, encode_frame};
use sp_security::{Fingerprint, public_key_from_cert};

use crate::TransportError;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PathStats {
    pub rtt: Duration,
    pub lost_packets: u64,
    pub sent_packets: u64,
    pub congestion_window: u64,
}

#[derive(Clone)]
pub struct SecureConnection {
    conn: quinn::Connection,
    peer_key: [u8; 32],
}

impl SecureConnection {
    pub(crate) fn new(conn: quinn::Connection) -> Result<Self, TransportError> {
        let certs = conn
            .peer_identity()
            .and_then(|id| id.downcast::<Vec<CertificateDer<'static>>>().ok())
            .ok_or(TransportError::PeerIdentity)?;
        let leaf = certs.first().ok_or(TransportError::PeerIdentity)?;
        let peer_key = public_key_from_cert(leaf.as_ref()).map_err(|_| TransportError::PeerIdentity)?;
        Ok(Self { conn, peer_key })
    }

    pub fn peer_public_key(&self) -> [u8; 32] {
        self.peer_key
    }

    pub fn peer_fingerprint(&self) -> Fingerprint {
        Fingerprint::of_public_key(&self.peer_key)
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.conn.remote_address()
    }

    /// Stable id for this connection instance.
    pub fn stable_id(&self) -> usize {
        self.conn.stable_id()
    }

    /// Open the control stream (dialer side). The dialer must send first.
    pub async fn open_control(&self) -> Result<(ControlSender, ControlReceiver), TransportError> {
        let (send, recv) = self.conn.open_bi().await?;
        Ok((ControlSender { send }, ControlReceiver::new(recv)))
    }

    /// Accept the control stream (listener side).
    pub async fn accept_control(&self) -> Result<(ControlSender, ControlReceiver), TransportError> {
        let (send, recv) = self.conn.accept_bi().await?;
        Ok((ControlSender { send }, ControlReceiver::new(recv)))
    }

    /// Send one media datagram. Dropped silently by QUIC if congested (by design).
    pub fn send_datagram(&self, data: Bytes) -> Result<(), TransportError> {
        self.conn.send_datagram(data)?;
        Ok(())
    }

    pub async fn read_datagram(&self) -> Result<Bytes, TransportError> {
        Ok(self.conn.read_datagram().await?)
    }

    pub fn max_datagram_size(&self) -> Option<usize> {
        self.conn.max_datagram_size()
    }

    pub fn stats(&self) -> PathStats {
        let s = self.conn.stats();
        PathStats {
            rtt: self.conn.rtt(),
            lost_packets: s.path.lost_packets,
            sent_packets: s.path.sent_packets,
            congestion_window: s.path.cwnd,
        }
    }

    /// Keying material bound to this TLS session, used to derive the pairing SAS.
    pub fn export_keying_material(&self, label: &[u8], context: &[u8]) -> Result<[u8; 32], TransportError> {
        let mut out = [0u8; 32];
        self.conn
            .export_keying_material(&mut out, label, context)
            .map_err(|_| TransportError::Tls("keying material export failed".into()))?;
        Ok(out)
    }

    pub fn close(&self, code: u32, reason: &[u8]) {
        self.conn.close(code.into(), reason);
    }

    pub async fn closed(&self) -> quinn::ConnectionError {
        self.conn.closed().await
    }
}

pub struct ControlSender {
    send: quinn::SendStream,
}

impl ControlSender {
    pub async fn send(&mut self, msg: &ControlMsg) -> Result<(), TransportError> {
        let mut buf = BytesMut::new();
        encode_frame(&msg.to_bytes(), MAX_CONTROL_FRAME, &mut buf)?;
        self.send.write_all(&buf).await?;
        Ok(())
    }

    pub fn finish(&mut self) {
        let _ = self.send.finish();
    }
}

pub struct ControlReceiver {
    recv: quinn::RecvStream,
    decoder: FrameDecoder,
    buf: Vec<u8>,
}

impl ControlReceiver {
    fn new(recv: quinn::RecvStream) -> Self {
        Self {
            recv,
            decoder: FrameDecoder::new(MAX_CONTROL_FRAME),
            buf: vec![0u8; 8192],
        }
    }

    /// Receive the next control message. Errors on malformed input or closure.
    pub async fn recv(&mut self) -> Result<ControlMsg, TransportError> {
        loop {
            if let Some(frame) = self.decoder.next_frame()? {
                return Ok(ControlMsg::from_bytes(&frame)?);
            }
            match self.recv.read(&mut self.buf).await? {
                Some(n) => self.decoder.extend(&self.buf[..n]),
                None => return Err(TransportError::Closed),
            }
        }
    }

    pub async fn recv_timeout(&mut self, timeout: Duration) -> Result<ControlMsg, TransportError> {
        tokio::time::timeout(timeout, self.recv())
            .await
            .map_err(|_| TransportError::Timeout)?
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use sp_protocol::control::{Ping, control_msg::Body};
    use sp_security::DeviceIdentity;

    use crate::{Endpoint, EndpointConfig};

    fn config() -> EndpointConfig {
        EndpointConfig {
            preferred_port: 0,
            ..EndpointConfig::default()
        }
    }

    #[tokio::test]
    async fn mutual_auth_control_and_datagrams() {
        let server_id = DeviceIdentity::generate();
        let client_id = DeviceIdentity::generate();
        let server = Endpoint::bind(&server_id, &config()).unwrap();
        let client = Endpoint::bind(&client_id, &config()).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", server.local_port()).parse().unwrap();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap().finish().await.unwrap();
            let (mut tx, mut rx) = conn.accept_control().await.unwrap();
            let msg = rx.recv().await.unwrap();
            tx.send(&msg).await.unwrap();
            let dgram = conn.read_datagram().await.unwrap();
            conn.send_datagram(dgram).unwrap();
            let sas = conn.export_keying_material(b"test", b"").unwrap();
            // Keep the connection alive until the client has read the echo.
            let _ = conn.closed().await;
            (conn.peer_public_key(), sas, server)
        });

        let conn = client.connect(addr, Some(server_id.device_id())).await.unwrap();
        assert_eq!(conn.peer_public_key(), server_id.public_key());

        let (mut tx, mut rx) = conn.open_control().await.unwrap();
        let ping = sp_protocol::control::ControlMsg::new(5, Body::Ping(Ping { nonce: 9, sent_us: 1 }));
        tx.send(&ping).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), ping);

        conn.send_datagram(bytes::Bytes::from_static(b"media")).unwrap();
        assert_eq!(&conn.read_datagram().await.unwrap()[..], b"media");

        let client_sas = conn.export_keying_material(b"test", b"").unwrap();
        conn.close(0, b"done");
        let (seen_client_key, server_sas, _server) = server_task.await.unwrap();
        assert_eq!(seen_client_key, client_id.public_key());
        assert_eq!(client_sas, server_sas);
    }

    #[tokio::test]
    async fn wrong_pinned_device_id_is_rejected() {
        let server_id = DeviceIdentity::generate();
        let server = Endpoint::bind(&server_id, &config()).unwrap();
        let client = Endpoint::bind(&DeviceIdentity::generate(), &config()).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", server.local_port()).parse().unwrap();

        tokio::spawn(async move {
            if let Some(handshake) = server.accept().await {
                let _ = handshake.finish().await;
            }
        });
        let wrong = DeviceIdentity::generate().device_id();
        assert!(client.connect(addr, Some(wrong)).await.is_err());
    }
}
