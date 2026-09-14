//! TLS 1.3 over TCP, for USB via `adb reverse` (which forwards TCP only). See ADR-0005.
//!
//! One TLS stream multiplexes typed frames ([`StreamFrameKind`]): control messages, media
//! datagrams, keep-alives and close. Identity and pinning are exactly those of the QUIC
//! transport (same certificates, same verifiers); the engine checks the peer key against its
//! trust store after the handshake either way.
//!
//! Datagram semantics are preserved: the send queue is small and drops new media when full, and
//! the writer discards media that waited longer than [`STALE_AFTER`], so a backed-up socket loses
//! audio instead of delaying it.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use rustls::pki_types::ServerName;
use sp_protocol::ProtocolError;
use sp_protocol::framing::{
    MAX_MEDIA_FRAME, StreamFrameDecoder, StreamFrameKind, encode_stream_frame,
};
use sp_security::{DeviceId, DeviceIdentity, public_key_from_cert};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{debug, warn};

use crate::TransportError;
use crate::connection::SecureConnection;
use crate::tls::{Credentials, SERVER_NAME};

/// ALPN for the TCP transport (distinct from QUIC's so the two can never be confused).
pub const ALPN_TCP: &[u8] = b"soundpush-tcp/1";
/// Exporter values the engine needs (pairing SAS), computed before the stream is split.
pub(crate) const EXPORTED_LABELS: &[(&[u8], &[u8])] =
    &[(sp_security::pairing::SAS_EXPORTER_LABEL, b"")];

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const KEEP_ALIVE: Duration = Duration::from_secs(1);
const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
/// Media older than this when the socket accepts it again is dropped, not sent late.
pub const STALE_AFTER: Duration = Duration::from_millis(200);
const CONTROL_QUEUE: usize = 256;
const DATAGRAM_SEND_QUEUE: usize = 64;
const DATAGRAM_RECV_QUEUE: usize = 256;

/// Listens for and dials TLS-over-TCP connections.
pub struct TcpEndpoint {
    credentials: Credentials,
    acceptor: TlsAcceptor,
    listener: Option<TcpListener>,
    shutdown: watch::Sender<bool>,
}

impl TcpEndpoint {
    /// Listen on `ip`, preferring `preferred_port` and falling back to an ephemeral port.
    /// The engine listens on loopback only: `adb reverse` delivers connections there.
    pub async fn bind(
        identity: &DeviceIdentity,
        ip: IpAddr,
        preferred_port: u16,
    ) -> Result<Self, TransportError> {
        let listener = match TcpListener::bind((ip, preferred_port)).await {
            Ok(l) => l,
            Err(e) if preferred_port != 0 => {
                warn!(preferred_port, error = %e, "preferred TCP port busy, using an ephemeral port");
                TcpListener::bind((ip, 0))
                    .await
                    .map_err(TransportError::Bind)?
            }
            Err(e) => return Err(TransportError::Bind(e)),
        };
        Self::new(identity, Some(listener))
    }

    /// An endpoint that only dials.
    pub fn dialer(identity: &DeviceIdentity) -> Result<Self, TransportError> {
        Self::new(identity, None)
    }

    fn new(
        identity: &DeviceIdentity,
        listener: Option<TcpListener>,
    ) -> Result<Self, TransportError> {
        let credentials = Credentials::new(identity)?;
        let acceptor = TlsAcceptor::from(Arc::new(credentials.server_config(ALPN_TCP)?));
        Ok(Self {
            credentials,
            acceptor,
            listener,
            shutdown: watch::channel(false).0,
        })
    }

    /// Listening port, or 0 for a dial-only endpoint.
    pub fn local_port(&self) -> u16 {
        self.listener
            .as_ref()
            .and_then(|l| l.local_addr().ok())
            .map(|a| a.port())
            .unwrap_or(0)
    }

    /// Wait for the next TCP connection. `None` when closed or dial-only.
    pub async fn accept(&self) -> Option<TcpHandshake> {
        let listener = self.listener.as_ref()?;
        let mut shutdown = self.shutdown.subscribe();
        loop {
            if *shutdown.borrow_and_update() {
                return None;
            }
            tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok((stream, remote)) => {
                        return Some(TcpHandshake {
                            stream,
                            remote,
                            acceptor: self.acceptor.clone(),
                        });
                    }
                    Err(e) => {
                        // e.g. out of file descriptors: back off instead of spinning.
                        debug!(error = %e, "tcp accept failed");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                _ = shutdown.changed() => return None,
            }
        }
    }

    /// Dial a peer over TCP. With `pinned`, the handshake fails unless the peer's key has that device ID.
    pub async fn connect(
        &self,
        addr: SocketAddr,
        pinned: Option<DeviceId>,
    ) -> Result<SecureConnection, TransportError> {
        let connector =
            TlsConnector::from(Arc::new(self.credentials.client_config(pinned, ALPN_TCP)?));
        let name =
            ServerName::try_from(SERVER_NAME).map_err(|e| TransportError::Tls(e.to_string()))?;
        tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
            let stream = TcpStream::connect(addr).await?;
            stream.set_nodelay(true)?;
            let tls = connector.connect(name, stream).await?;
            let (peer_key, exported) = session_info(tls.get_ref().1)?;
            Ok(start(tls, addr, peer_key, exported))
        })
        .await
        .map_err(|_| TransportError::Timeout)?
    }

    pub fn close(&self) {
        self.shutdown.send_replace(true);
    }
}

/// An accepted TCP connection whose TLS handshake has not run yet.
pub struct TcpHandshake {
    stream: TcpStream,
    remote: SocketAddr,
    acceptor: TlsAcceptor,
}

impl TcpHandshake {
    pub fn remote_address(&self) -> SocketAddr {
        self.remote
    }

    /// Complete the TLS handshake (mutual authentication).
    pub async fn finish(self) -> Result<SecureConnection, TransportError> {
        let remote = self.remote;
        tokio::time::timeout(HANDSHAKE_TIMEOUT, async move {
            self.stream.set_nodelay(true)?;
            let tls = self.acceptor.accept(self.stream).await?;
            let (peer_key, exported) = session_info(tls.get_ref().1)?;
            Ok(start(tls, remote, peer_key, exported))
        })
        .await
        .map_err(|_| TransportError::Timeout)?
        .inspect_err(|e| debug!(%remote, error = %e, "incoming tcp handshake failed"))
    }
}

type Exported = Vec<(&'static [u8], &'static [u8], [u8; 32])>;

fn session_info<D>(
    conn: &rustls::ConnectionCommon<D>,
) -> Result<([u8; 32], Exported), TransportError> {
    let leaf = conn
        .peer_certificates()
        .and_then(|certs| certs.first())
        .ok_or(TransportError::PeerIdentity)?;
    let peer_key = public_key_from_cert(leaf.as_ref()).map_err(|_| TransportError::PeerIdentity)?;
    let mut exported = Vec::with_capacity(EXPORTED_LABELS.len());
    for (label, context) in EXPORTED_LABELS {
        let out = conn
            .export_keying_material([0u8; 32], label, Some(context))
            .map_err(|_| TransportError::Tls("keying material export failed".into()))?;
        exported.push((*label, *context, out));
    }
    Ok((peer_key, exported))
}

/// Frames queued for the writer task (control has priority over media).
pub(crate) enum Outbound {
    Control(Bytes),
    Pong([u8; 8]),
    Close(u32),
}

pub(crate) struct TcpStats {
    epoch: Instant,
    pub rtt_us: AtomicU64,
    pub sent: AtomicU64,
    pub lost: AtomicU64,
}

impl TcpStats {
    fn now_us(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64
    }
}

/// State shared by every clone of a TCP [`SecureConnection`]. Dropping the last clone ends the
/// I/O tasks (their queues close), mirroring how QUIC closes a connection nobody holds.
pub(crate) struct TcpShared {
    pub remote: SocketAddr,
    control_out: mpsc::Sender<Outbound>,
    datagram_out: mpsc::Sender<(Instant, Bytes)>,
    control_in: Mutex<Option<mpsc::Receiver<Bytes>>>,
    datagram_in: tokio::sync::Mutex<mpsc::Receiver<Bytes>>,
    exported: Exported,
    pub stats: Arc<TcpStats>,
    closed: Arc<watch::Sender<bool>>,
}

impl TcpShared {
    /// The control channel. Both sides take it once (TCP has no stream to open).
    pub fn take_control(&self) -> Option<(mpsc::Sender<Outbound>, mpsc::Receiver<Bytes>)> {
        let rx = self.control_in.lock().ok()?.take()?;
        Some((self.control_out.clone(), rx))
    }

    pub fn send_datagram(&self, data: Bytes) -> Result<(), TransportError> {
        if data.len() > MAX_MEDIA_FRAME {
            return Err(ProtocolError::FrameTooLarge {
                len: data.len(),
                max: MAX_MEDIA_FRAME,
            }
            .into());
        }
        match self.datagram_out.try_send((Instant::now(), data)) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                // Congested: drop, like QUIC does with datagrams.
                self.stats.lost.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(TransportError::Closed),
        }
    }

    pub async fn read_datagram(&self) -> Result<Bytes, TransportError> {
        self.datagram_in
            .lock()
            .await
            .recv()
            .await
            .ok_or(TransportError::Closed)
    }

    pub fn exported(&self, label: &[u8], context: &[u8]) -> Option<[u8; 32]> {
        self.exported
            .iter()
            .find(|(l, c, _)| *l == label && *c == context)
            .map(|(_, _, v)| *v)
    }

    pub fn close(&self, code: u32) {
        if self.control_out.try_send(Outbound::Close(code)).is_err() {
            self.closed.send_replace(true);
        }
    }

    pub async fn closed(&self) {
        let mut rx = self.closed.subscribe();
        let _ = rx.wait_for(|closed| *closed).await;
    }

    pub fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }
}

fn start<S>(
    stream: S,
    remote: SocketAddr,
    peer_key: [u8; 32],
    exported: Exported,
) -> SecureConnection
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let (control_out, control_out_rx) = mpsc::channel(CONTROL_QUEUE);
    let (datagram_out, datagram_out_rx) = mpsc::channel(DATAGRAM_SEND_QUEUE);
    let (control_in_tx, control_in) = mpsc::channel(CONTROL_QUEUE);
    let (datagram_in_tx, datagram_in) = mpsc::channel(DATAGRAM_RECV_QUEUE);
    let closed = Arc::new(watch::channel(false).0);
    let stats = Arc::new(TcpStats {
        epoch: Instant::now(),
        rtt_us: AtomicU64::new(0),
        sent: AtomicU64::new(0),
        lost: AtomicU64::new(0),
    });

    tokio::spawn(write_loop(
        writer,
        control_out_rx,
        datagram_out_rx,
        stats.clone(),
        closed.clone(),
    ));
    tokio::spawn(read_loop(
        reader,
        control_in_tx,
        datagram_in_tx,
        control_out.downgrade(),
        stats.clone(),
        closed.clone(),
    ));

    SecureConnection::from_tcp(
        Arc::new(TcpShared {
            remote,
            control_out,
            datagram_out,
            control_in: Mutex::new(Some(control_in)),
            datagram_in: tokio::sync::Mutex::new(datagram_in),
            exported,
            stats,
            closed,
        }),
        peer_key,
    )
}

async fn write_loop<W: AsyncWrite + Unpin>(
    mut writer: W,
    mut control: mpsc::Receiver<Outbound>,
    mut datagrams: mpsc::Receiver<(Instant, Bytes)>,
    stats: Arc<TcpStats>,
    closed: Arc<watch::Sender<bool>>,
) {
    let mut closed_rx = closed.subscribe();
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut buf = BytesMut::with_capacity(4096);
    loop {
        if *closed_rx.borrow_and_update() {
            break;
        }
        buf.clear();
        let mut finish = false;
        let encoded = tokio::select! {
            biased;
            _ = closed_rx.changed() => break,
            msg = control.recv() => match msg {
                Some(Outbound::Control(bytes)) => encode_stream_frame(StreamFrameKind::Control, &bytes, &mut buf),
                Some(Outbound::Pong(payload)) => encode_stream_frame(StreamFrameKind::Pong, &payload, &mut buf),
                Some(Outbound::Close(code)) => {
                    finish = true;
                    encode_stream_frame(StreamFrameKind::Close, &code.to_be_bytes(), &mut buf)
                }
                None => break,
            },
            item = datagrams.recv() => match item {
                Some((queued, bytes)) => {
                    if queued.elapsed() > STALE_AFTER {
                        stats.lost.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    stats.sent.fetch_add(1, Ordering::Relaxed);
                    encode_stream_frame(StreamFrameKind::Datagram, &bytes, &mut buf)
                }
                None => break,
            },
            _ = keep_alive.tick() => encode_stream_frame(StreamFrameKind::Ping, &stats.now_us().to_be_bytes(), &mut buf),
        };
        if encoded.is_err() {
            continue;
        }
        if writer.write_all(&buf).await.is_err() || writer.flush().await.is_err() || finish {
            break;
        }
    }
    let _ = writer.shutdown().await;
    closed.send_replace(true);
}

async fn read_loop<R: AsyncRead + Unpin>(
    mut reader: R,
    control_in: mpsc::Sender<Bytes>,
    datagram_in: mpsc::Sender<Bytes>,
    control_out: mpsc::WeakSender<Outbound>,
    stats: Arc<TcpStats>,
    closed: Arc<watch::Sender<bool>>,
) {
    let mut closed_rx = closed.subscribe();
    let mut decoder = StreamFrameDecoder::new();
    let mut buf = vec![0u8; 16 * 1024];
    'read: loop {
        if *closed_rx.borrow_and_update() {
            break;
        }
        // Keep-alives arrive every second; ten seconds of silence means the peer is gone.
        let n = tokio::select! {
            read = tokio::time::timeout(IDLE_TIMEOUT, reader.read(&mut buf)) => match read {
                Ok(Ok(n)) if n > 0 => n,
                _ => break,
            },
            _ = closed_rx.changed() => break,
        };
        decoder.extend(&buf[..n]);
        loop {
            match decoder.next_frame() {
                Ok(Some((StreamFrameKind::Control, payload))) => {
                    if control_in.send(payload).await.is_err() {
                        break 'read;
                    }
                }
                Ok(Some((StreamFrameKind::Datagram, payload))) => {
                    // Full: the session is not keeping up; dropping media is correct.
                    if let Err(mpsc::error::TrySendError::Closed(_)) = datagram_in.try_send(payload)
                    {
                        break 'read;
                    }
                }
                Ok(Some((StreamFrameKind::Ping, payload))) => {
                    let (Ok(echo), Some(out)) =
                        (<[u8; 8]>::try_from(&payload[..]), control_out.upgrade())
                    else {
                        break 'read;
                    };
                    let _ = out.try_send(Outbound::Pong(echo));
                }
                Ok(Some((StreamFrameKind::Pong, payload))) => {
                    if let Ok(sent) = <[u8; 8]>::try_from(&payload[..]) {
                        let sample = stats.now_us().saturating_sub(u64::from_be_bytes(sent));
                        let old = stats.rtt_us.load(Ordering::Relaxed);
                        let rtt = if old == 0 {
                            sample
                        } else {
                            (old * 7 + sample) / 8
                        };
                        stats.rtt_us.store(rtt, Ordering::Relaxed);
                    }
                }
                Ok(Some((StreamFrameKind::Close, _))) => break 'read,
                Ok(None) => break,
                Err(e) => {
                    debug!(error = %e, "corrupt tcp stream");
                    break 'read;
                }
            }
        }
    }
    closed.send_replace(true);
}
