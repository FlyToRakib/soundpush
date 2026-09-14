//! An authenticated connection to a peer, over QUIC or TLS-over-TCP.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use rustls::pki_types::CertificateDer;
use sp_protocol::ProtocolError;
use sp_protocol::control::ControlMsg;
use sp_protocol::framing::{FrameDecoder, MAX_CONTROL_FRAME, MAX_MEDIA_FRAME, encode_frame};
use sp_security::{Fingerprint, public_key_from_cert};
use tokio::sync::mpsc;

use crate::TransportError;
use crate::qos::Flow;
use crate::tcp::{Outbound, TcpShared};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PathStats {
    pub rtt: Duration,
    pub lost_packets: u64,
    pub sent_packets: u64,
    pub congestion_window: u64,
}

/// Which transport carries a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Quic,
    /// TLS over TCP (USB via `adb reverse`).
    Tcp,
}

#[derive(Clone)]
enum Inner {
    Quic(quinn::Connection),
    Tcp(Arc<TcpShared>),
}

#[derive(Clone)]
pub struct SecureConnection {
    inner: Inner,
    peer_key: [u8; 32],
    /// DSCP marking of this peer's traffic while any clone is alive (Windows qWAVE flow).
    _qos: Option<Arc<Flow>>,
}

impl SecureConnection {
    pub(crate) fn from_quic(
        conn: quinn::Connection,
        qos: Option<Arc<Flow>>,
    ) -> Result<Self, TransportError> {
        let certs = conn
            .peer_identity()
            .and_then(|id| id.downcast::<Vec<CertificateDer<'static>>>().ok())
            .ok_or(TransportError::PeerIdentity)?;
        let leaf = certs.first().ok_or(TransportError::PeerIdentity)?;
        let peer_key =
            public_key_from_cert(leaf.as_ref()).map_err(|_| TransportError::PeerIdentity)?;
        Ok(Self {
            inner: Inner::Quic(conn),
            peer_key,
            _qos: qos,
        })
    }

    pub(crate) fn from_tcp(shared: Arc<TcpShared>, peer_key: [u8; 32]) -> Self {
        Self {
            inner: Inner::Tcp(shared),
            peer_key,
            _qos: None,
        }
    }

    pub fn peer_public_key(&self) -> [u8; 32] {
        self.peer_key
    }

    pub fn peer_fingerprint(&self) -> Fingerprint {
        Fingerprint::of_public_key(&self.peer_key)
    }

    pub fn transport(&self) -> TransportKind {
        match &self.inner {
            Inner::Quic(_) => TransportKind::Quic,
            Inner::Tcp(_) => TransportKind::Tcp,
        }
    }

    /// The peer's current address. For QUIC this follows connection migration.
    pub fn remote_address(&self) -> SocketAddr {
        match &self.inner {
            Inner::Quic(c) => c.remote_address(),
            Inner::Tcp(t) => t.remote,
        }
    }

    /// Stable id for this connection instance.
    pub fn stable_id(&self) -> usize {
        match &self.inner {
            Inner::Quic(c) => c.stable_id(),
            Inner::Tcp(t) => Arc::as_ptr(t) as usize,
        }
    }

    /// Open the control stream (dialer side). The dialer must send first.
    pub async fn open_control(&self) -> Result<(ControlSender, ControlReceiver), TransportError> {
        match &self.inner {
            Inner::Quic(c) => {
                let (send, recv) = c.open_bi().await?;
                Ok((ControlSender::quic(send), ControlReceiver::quic(recv)))
            }
            Inner::Tcp(t) => tcp_control(t),
        }
    }

    /// Accept the control stream (listener side).
    pub async fn accept_control(&self) -> Result<(ControlSender, ControlReceiver), TransportError> {
        match &self.inner {
            Inner::Quic(c) => {
                let (send, recv) = c.accept_bi().await?;
                Ok((ControlSender::quic(send), ControlReceiver::quic(recv)))
            }
            Inner::Tcp(t) => tcp_control(t),
        }
    }

    /// Send one media datagram. Dropped silently if congested (by design).
    pub fn send_datagram(&self, data: Bytes) -> Result<(), TransportError> {
        match &self.inner {
            Inner::Quic(c) => Ok(c.send_datagram(data)?),
            Inner::Tcp(t) => t.send_datagram(data),
        }
    }

    pub async fn read_datagram(&self) -> Result<Bytes, TransportError> {
        match &self.inner {
            Inner::Quic(c) => Ok(c.read_datagram().await?),
            Inner::Tcp(t) => t.read_datagram().await,
        }
    }

    pub fn max_datagram_size(&self) -> Option<usize> {
        match &self.inner {
            Inner::Quic(c) => c.max_datagram_size(),
            Inner::Tcp(_) => Some(MAX_MEDIA_FRAME),
        }
    }

    pub fn stats(&self) -> PathStats {
        match &self.inner {
            Inner::Quic(c) => {
                let s = c.stats();
                PathStats {
                    rtt: c.rtt(),
                    lost_packets: s.path.lost_packets,
                    sent_packets: s.path.sent_packets,
                    congestion_window: s.path.cwnd,
                }
            }
            Inner::Tcp(t) => PathStats {
                rtt: Duration::from_micros(t.stats.rtt_us.load(Ordering::Relaxed)),
                lost_packets: t.stats.lost.load(Ordering::Relaxed),
                sent_packets: t.stats.sent.load(Ordering::Relaxed),
                congestion_window: 0,
            },
        }
    }

    /// Keying material bound to this TLS session, used to derive the pairing SAS.
    /// Over TCP only the labels in `tcp::EXPORTED_LABELS` are available.
    pub fn export_keying_material(
        &self,
        label: &[u8],
        context: &[u8],
    ) -> Result<[u8; 32], TransportError> {
        let failed = || TransportError::Tls("keying material export failed".into());
        match &self.inner {
            Inner::Quic(c) => {
                let mut out = [0u8; 32];
                c.export_keying_material(&mut out, label, context)
                    .map_err(|_| failed())?;
                Ok(out)
            }
            Inner::Tcp(t) => t.exported(label, context).ok_or_else(failed),
        }
    }

    pub fn close(&self, code: u32, reason: &[u8]) {
        match &self.inner {
            Inner::Quic(c) => c.close(code.into(), reason),
            Inner::Tcp(t) => t.close(code),
        }
    }

    /// Resolves once the connection is closed (by either side or by timeout).
    pub async fn closed(&self) {
        match &self.inner {
            Inner::Quic(c) => {
                c.closed().await;
            }
            Inner::Tcp(t) => t.closed().await,
        }
    }

    pub fn is_closed(&self) -> bool {
        match &self.inner {
            Inner::Quic(c) => c.close_reason().is_some(),
            Inner::Tcp(t) => t.is_closed(),
        }
    }
}

fn tcp_control(t: &TcpShared) -> Result<(ControlSender, ControlReceiver), TransportError> {
    let (tx, rx) = t.take_control().ok_or(TransportError::Closed)?;
    Ok((
        ControlSender {
            inner: SenderInner::Tcp(tx),
        },
        ControlReceiver {
            inner: ReceiverInner::Tcp(rx),
        },
    ))
}

pub struct ControlSender {
    inner: SenderInner,
}

enum SenderInner {
    Quic(quinn::SendStream),
    Tcp(mpsc::Sender<Outbound>),
}

impl ControlSender {
    fn quic(send: quinn::SendStream) -> Self {
        Self {
            inner: SenderInner::Quic(send),
        }
    }

    pub async fn send(&mut self, msg: &ControlMsg) -> Result<(), TransportError> {
        match &mut self.inner {
            SenderInner::Quic(send) => {
                let mut buf = BytesMut::new();
                encode_frame(&msg.to_bytes(), MAX_CONTROL_FRAME, &mut buf)?;
                send.write_all(&buf).await?;
            }
            SenderInner::Tcp(tx) => {
                let bytes = msg.to_bytes();
                if bytes.len() > MAX_CONTROL_FRAME {
                    return Err(ProtocolError::FrameTooLarge {
                        len: bytes.len(),
                        max: MAX_CONTROL_FRAME,
                    }
                    .into());
                }
                tx.send(Outbound::Control(bytes))
                    .await
                    .map_err(|_| TransportError::Closed)?;
            }
        }
        Ok(())
    }

    pub fn finish(&mut self) {
        if let SenderInner::Quic(send) = &mut self.inner {
            let _ = send.finish();
        }
    }
}

pub struct ControlReceiver {
    inner: ReceiverInner,
}

enum ReceiverInner {
    Quic {
        recv: quinn::RecvStream,
        decoder: FrameDecoder,
        buf: Vec<u8>,
    },
    Tcp(mpsc::Receiver<Bytes>),
}

impl ControlReceiver {
    fn quic(recv: quinn::RecvStream) -> Self {
        Self {
            inner: ReceiverInner::Quic {
                recv,
                decoder: FrameDecoder::new(MAX_CONTROL_FRAME),
                buf: vec![0u8; 8192],
            },
        }
    }

    async fn next_frame(&mut self) -> Result<Bytes, TransportError> {
        match &mut self.inner {
            ReceiverInner::Quic { recv, decoder, buf } => loop {
                if let Some(frame) = decoder.next_frame()? {
                    return Ok(frame);
                }
                match recv.read(buf).await? {
                    Some(n) => decoder.extend(&buf[..n]),
                    None => return Err(TransportError::Closed),
                }
            },
            ReceiverInner::Tcp(rx) => rx.recv().await.ok_or(TransportError::Closed),
        }
    }

    /// Receive the next control message. Errors on malformed input or closure. Messages of a
    /// type this build does not know (a newer peer) are skipped.
    pub async fn recv(&mut self) -> Result<ControlMsg, TransportError> {
        loop {
            let frame = self.next_frame().await?;
            match ControlMsg::from_bytes(&frame) {
                Ok(msg) => return Ok(msg),
                Err(ProtocolError::UnknownMessage) => continue,
                Err(e) => return Err(e.into()),
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
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use sp_protocol::control::{Ping, control_msg::Body};
    use sp_security::DeviceIdentity;
    use sp_security::pairing::SAS_EXPORTER_LABEL;

    use super::*;
    use crate::{Endpoint, EndpointConfig, TcpEndpoint};

    fn config() -> EndpointConfig {
        EndpointConfig {
            preferred_port: 0,
            ..EndpointConfig::default()
        }
    }

    fn ping(nonce: u64) -> ControlMsg {
        ControlMsg::new(5, Body::Ping(Ping { nonce, sent_us: 1 }))
    }

    /// Echo one control message and every datagram until the connection closes.
    async fn echo_server(conn: SecureConnection) -> ([u8; 32], [u8; 32]) {
        let (mut tx, mut rx) = conn.accept_control().await.unwrap();
        let sas = conn
            .export_keying_material(SAS_EXPORTER_LABEL, b"")
            .unwrap();
        let echo = conn.clone();
        tokio::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                if tx.send(&msg).await.is_err() {
                    break;
                }
            }
        });
        let dgrams = conn.clone();
        tokio::spawn(async move {
            while let Ok(d) = dgrams.read_datagram().await {
                let _ = echo.send_datagram(d);
            }
        });
        conn.closed().await;
        (conn.peer_public_key(), sas)
    }

    async fn exercise(conn: &SecureConnection) {
        let (mut tx, mut rx) = conn.open_control().await.unwrap();
        tx.send(&ping(9)).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), ping(9));
        conn.send_datagram(Bytes::from_static(b"media")).unwrap();
        assert_eq!(&conn.read_datagram().await.unwrap()[..], b"media");
    }

    #[tokio::test]
    async fn quic_mutual_auth_control_and_datagrams() {
        let server_id = DeviceIdentity::generate();
        let client_id = DeviceIdentity::generate();
        let server = Endpoint::bind(&server_id, &config()).unwrap();
        let client = Endpoint::bind(&client_id, &config()).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", server.local_port())
            .parse()
            .unwrap();

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap().finish().await.unwrap();
            let result = echo_server(conn).await;
            (result, server)
        });

        let conn = client
            .connect(addr, Some(server_id.device_id()))
            .await
            .unwrap();
        assert_eq!(conn.peer_public_key(), server_id.public_key());
        assert_eq!(conn.transport(), TransportKind::Quic);
        exercise(&conn).await;

        let client_sas = conn
            .export_keying_material(SAS_EXPORTER_LABEL, b"")
            .unwrap();
        conn.close(0, b"done");
        let ((seen_client_key, server_sas), _server) = server_task.await.unwrap();
        assert_eq!(seen_client_key, client_id.public_key());
        assert_eq!(client_sas, server_sas);
    }

    #[tokio::test]
    async fn quic_session_survives_client_address_change() {
        let server_id = DeviceIdentity::generate();
        let server = Endpoint::bind(&server_id, &config()).unwrap();
        let client = Endpoint::bind(&DeviceIdentity::generate(), &config()).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", server.local_port())
            .parse()
            .unwrap();

        let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel();
        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap().finish().await.unwrap();
            let watcher = conn.clone();
            tokio::spawn(async move {
                loop {
                    let _ = seen_tx.send(watcher.remote_address());
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            });
            echo_server(conn).await;
            server
        });

        let conn = client
            .connect(addr, Some(server_id.device_id()))
            .await
            .unwrap();
        let (mut tx, mut rx) = conn.open_control().await.unwrap();
        tx.send(&ping(1)).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), ping(1));
        let before = seen_rx.recv().await.unwrap();

        // The client moves to a new socket (new source port): the same connection continues.
        client.rebind_for_test().unwrap();
        tx.send(&ping(2)).await.unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).await.unwrap(),
            ping(2)
        );
        conn.send_datagram(Bytes::from_static(b"after")).unwrap();
        assert_eq!(&conn.read_datagram().await.unwrap()[..], b"after");
        let mut after = before;
        while after == before {
            after = seen_rx.recv().await.unwrap();
        }
        assert_ne!(
            after.port(),
            before.port(),
            "server follows the migrated path"
        );
        conn.close(0, b"done");
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn wrong_pinned_device_id_is_rejected() {
        let server_id = DeviceIdentity::generate();
        let server = Endpoint::bind(&server_id, &config()).unwrap();
        let client = Endpoint::bind(&DeviceIdentity::generate(), &config()).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", server.local_port())
            .parse()
            .unwrap();

        tokio::spawn(async move {
            if let Some(handshake) = server.accept().await {
                let _ = handshake.finish().await;
            }
        });
        let wrong = DeviceIdentity::generate().device_id();
        assert!(client.connect(addr, Some(wrong)).await.is_err());
    }

    #[tokio::test]
    async fn tcp_mutual_auth_control_datagrams_and_close() {
        let server_id = DeviceIdentity::generate();
        let client_id = DeviceIdentity::generate();
        let server = TcpEndpoint::bind(&server_id, IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
            .await
            .unwrap();
        let client = TcpEndpoint::dialer(&client_id).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.local_port()));

        let server_task = tokio::spawn(async move {
            let conn = server.accept().await.unwrap().finish().await.unwrap();
            assert_eq!(conn.transport(), TransportKind::Tcp);
            let result = echo_server(conn).await;
            (result, server)
        });

        let conn = client
            .connect(addr, Some(server_id.device_id()))
            .await
            .unwrap();
        assert_eq!(conn.peer_public_key(), server_id.public_key());
        exercise(&conn).await;
        // Media larger than the frame limit is refused, not truncated.
        assert!(
            conn.send_datagram(Bytes::from(vec![0u8; MAX_MEDIA_FRAME + 1]))
                .is_err()
        );

        let client_sas = conn
            .export_keying_material(SAS_EXPORTER_LABEL, b"")
            .unwrap();
        assert!(conn.export_keying_material(b"other", b"").is_err());
        conn.close(0, b"done");
        let ((seen_client_key, server_sas), _server) =
            tokio::time::timeout(Duration::from_secs(5), server_task)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(seen_client_key, client_id.public_key());
        assert_eq!(client_sas, server_sas);
        assert!(conn.is_closed());
    }

    #[tokio::test]
    async fn tcp_wrong_pin_is_rejected_and_idle_keepalive_holds() {
        let server_id = DeviceIdentity::generate();
        let server = TcpEndpoint::bind(&server_id, IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
            .await
            .unwrap();
        let client = TcpEndpoint::dialer(&DeviceIdentity::generate()).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.local_port()));
        let accepted = tokio::spawn(async move {
            let mut conns = Vec::new();
            while let Some(h) = server.accept().await {
                if let Ok(c) = h.finish().await {
                    conns.push(c);
                }
                if conns.len() == 1 {
                    return (conns, server);
                }
            }
            (conns, server)
        });

        assert!(
            client
                .connect(addr, Some(DeviceIdentity::generate().device_id()))
                .await
                .is_err()
        );
        let conn = client
            .connect(addr, Some(server_id.device_id()))
            .await
            .unwrap();
        let (conns, _server) = accepted.await.unwrap();
        // Keep-alives measure RTT and keep an idle link open.
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(!conn.is_closed() && !conns[0].is_closed());
        assert!(conn.stats().rtt > Duration::ZERO);
        drop(conns);
        tokio::time::timeout(Duration::from_secs(5), conn.closed())
            .await
            .unwrap();
    }
}
