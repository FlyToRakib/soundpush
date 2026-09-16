//! QUIC endpoint: listens for peers and dials them.

use std::net::{IpAddr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sp_security::{DeviceId, DeviceIdentity};
use tracing::{debug, warn};

use crate::connection::SecureConnection;
use crate::qos::{Flow, Marker};
use crate::tls::{Credentials, SERVER_NAME};
use crate::{ALPN, TransportError};

/// Preferred listening port (UDP). Falls back to an ephemeral port when busy.
pub const DEFAULT_PORT: u16 = 47650;

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub preferred_port: u16,
    pub keep_alive: Duration,
    pub idle_timeout: Duration,
    /// DSCP-mark media traffic as Expedited Forwarding where the platform allows (plan §16.3,
    /// `qos.rs`). Best effort: never fails the endpoint or a connection.
    pub dscp: bool,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            preferred_port: DEFAULT_PORT,
            keep_alive: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(10),
            dscp: true,
        }
    }
}

pub struct Endpoint {
    inner: quinn::Endpoint,
    credentials: Credentials,
    transport: Arc<quinn::TransportConfig>,
    /// True when bound to a dual-stack IPv6 socket (IPv4 targets are then IPv4-mapped).
    ipv6: bool,
    marker: Option<Arc<Marker>>,
}

impl Endpoint {
    /// Bind on all interfaces (dual-stack when available). Tries the preferred port first.
    pub fn bind(
        identity: &DeviceIdentity,
        config: &EndpointConfig,
    ) -> Result<Self, TransportError> {
        let credentials = Credentials::new(identity)?;

        let mut transport = quinn::TransportConfig::default();
        transport.keep_alive_interval(Some(config.keep_alive));
        transport.max_idle_timeout(Some(
            quinn::IdleTimeout::try_from(config.idle_timeout)
                .map_err(|e| TransportError::Tls(e.to_string()))?,
        ));
        transport.datagram_receive_buffer_size(Some(1 << 20));
        transport.datagram_send_buffer_size(1 << 20);
        let transport = Arc::new(transport);

        let quic_server =
            quinn::crypto::rustls::QuicServerConfig::try_from(credentials.server_config(ALPN)?)
                .map_err(|e| TransportError::Tls(e.to_string()))?;
        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_server));
        server_config.transport_config(transport.clone());
        // Connection migration: a peer whose address changes (Wi-Fi roam, DHCP renewal, switching
        // between networks within the idle timeout) keeps its session. The wildcard-bound socket
        // sends from whatever address the OS now routes through; quinn validates the new path.
        server_config.migration(true);

        let socket = bind_socket(config.preferred_port)?;
        let ipv6 = socket.local_addr().is_ok_and(|a| a.is_ipv6());
        let marker = config.dscp.then(|| Marker::for_socket(&socket));
        let runtime = quinn::default_runtime()
            .ok_or_else(|| TransportError::Tls("no async runtime".into()))?;
        let inner = quinn::Endpoint::new(
            quinn::EndpointConfig::default(),
            Some(server_config),
            socket,
            runtime,
        )
        .map_err(TransportError::Bind)?;

        Ok(Self {
            inner,
            credentials,
            transport,
            ipv6,
            marker,
        })
    }

    pub fn local_port(&self) -> u16 {
        self.inner.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    /// Dial a peer. With `pinned`, the handshake fails unless the peer's key has that device ID.
    pub async fn connect(
        &self,
        addr: SocketAddr,
        pinned: Option<DeviceId>,
    ) -> Result<SecureConnection, TransportError> {
        let dialed_self = Arc::new(AtomicBool::new(false));
        let quic_client = quinn::crypto::rustls::QuicClientConfig::try_from(
            self.credentials
                .client_config(pinned, ALPN, dialed_self.clone())?,
        )
        .map_err(|e| TransportError::Tls(e.to_string()))?;
        let mut client_config = quinn::ClientConfig::new(Arc::new(quic_client));
        client_config.transport_config(self.transport.clone());

        let addr = if self.ipv6 { normalize(addr) } else { addr };
        debug!(%addr, "dialing peer");
        let connecting = self.inner.connect_with(client_config, addr, SERVER_NAME)?;
        let conn = connecting.await.map_err(|e| {
            if dialed_self.load(Ordering::Relaxed) {
                TransportError::DialedSelf
            } else {
                TransportError::from(e)
            }
        })?;
        let flow = mark_peer(self.marker.as_ref(), conn.remote_address());
        SecureConnection::from_quic(conn, flow)
    }

    /// Wait for the next connection attempt. Returns `None` when the endpoint is closed.
    /// The handshake is completed by [`Handshake::finish`], so a caller can run several at
    /// once and one stalled peer does not hold up the others.
    pub async fn accept(&self) -> Option<Handshake> {
        self.inner.accept().await.map(|incoming| Handshake {
            incoming,
            marker: self.marker.clone(),
        })
    }

    pub fn close(&self) {
        self.inner.close(0u32.into(), b"shutdown");
    }

    /// Move the endpoint to another socket, as happens when a device changes networks.
    #[cfg(test)]
    pub(crate) fn rebind_for_test(&self) -> std::io::Result<()> {
        let socket = bind_socket(0).map_err(|e| std::io::Error::other(e.to_string()))?;
        self.inner.rebind(socket)
    }
}

/// An accepted connection attempt whose QUIC handshake has not run yet.
pub struct Handshake {
    incoming: quinn::Incoming,
    marker: Option<Arc<Marker>>,
}

fn mark_peer(marker: Option<&Arc<Marker>>, peer: SocketAddr) -> Option<Arc<Flow>> {
    marker.and_then(|m| m.mark_peer(peer))
}

impl Handshake {
    pub fn remote_address(&self) -> SocketAddr {
        self.incoming.remote_address()
    }

    /// Complete the handshake (TLS with mutual authentication).
    pub async fn finish(self) -> Result<SecureConnection, TransportError> {
        let remote = self.incoming.remote_address();
        match self.incoming.await {
            Ok(conn) => {
                let flow = mark_peer(self.marker.as_ref(), conn.remote_address());
                SecureConnection::from_quic(conn, flow)
            }
            Err(e) => {
                log_handshake_failure(remote, &e);
                Err(e.into())
            }
        }
    }
}

/// How long repeated failures from one address are counted instead of logged.
const HANDSHAKE_FAILURE_QUIET: Duration = Duration::from_secs(60);
/// Addresses tracked for that; far more than a home network ever has, and it is pruned.
const HANDSHAKE_FAILURE_ADDRESSES: usize = 64;

/// Failures per remote address: when the first one was logged, and how many followed it.
static HANDSHAKE_FAILURES: Mutex<Vec<(IpAddr, Instant, u32)>> = Mutex::new(Vec::new());

/// Log a failed incoming handshake, at most once a minute per address.
///
/// A peer that cannot complete the handshake retries on its own reconnect timer, so without this
/// one unreachable device writes a warning every few seconds for as long as it runs — enough to
/// push everything else out of a rotated log. The first failure is reported immediately; the ones
/// that follow are counted and summarized.
fn log_handshake_failure(remote: SocketAddr, error: &quinn::ConnectionError) {
    // The peer closed it on purpose. Two devices that reconnect at the same moment each dial the
    // other, and the engine keeps one connection and closes the duplicate — often while its
    // handshake is still running. That is the design working, not a failure worth a warning.
    // An application close cannot be sent before the handshake completes, so QUIC carries it as
    // a transport close with APPLICATION_ERROR; after it, as an ordinary application close.
    let closed_on_purpose = match error {
        quinn::ConnectionError::ApplicationClosed(_) => true,
        quinn::ConnectionError::ConnectionClosed(close) => {
            close.error_code == quinn::TransportErrorCode::APPLICATION_ERROR
        }
        _ => false,
    };
    if closed_on_purpose {
        debug!(%remote, %error, "incoming handshake closed by the peer");
        return;
    }
    let now = Instant::now();
    let Ok(mut seen) = HANDSHAKE_FAILURES.lock() else {
        warn!(%remote, %error, "incoming handshake failed");
        return;
    };
    seen.retain(|(_, at, _)| now.duration_since(*at) < HANDSHAKE_FAILURE_QUIET);
    if let Some((_, at, repeats)) = seen.iter_mut().find(|(ip, _, _)| *ip == remote.ip()) {
        *repeats += 1;
        if now.duration_since(*at) < HANDSHAKE_FAILURE_QUIET {
            return;
        }
        let repeats = std::mem::replace(repeats, 0);
        *at = now;
        warn!(%remote, %error, repeats, "incoming handshake keeps failing");
        return;
    }
    if seen.len() < HANDSHAKE_FAILURE_ADDRESSES {
        seen.push((remote.ip(), now, 0));
    }
    warn!(%remote, %error, "incoming handshake failed");
}

/// Bind a dual-stack IPv6 socket so one socket serves IPv4 and IPv6 peers.
/// Windows defaults IPV6_V6ONLY to true (Linux/macOS/Android default to false), so it
/// must be turned off explicitly or every IPv4 peer is unreachable.
fn bind_dual_stack(port: u16) -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_only_v6(false)?;
    socket.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)).into())?;
    Ok(socket.into())
}

fn bind_socket(preferred: u16) -> Result<UdpSocket, TransportError> {
    let candidates = [preferred, 0];
    let mut last_err = None;
    for port in candidates {
        // Fall back to IPv4-only where IPv6 is disabled.
        match bind_dual_stack(port)
            .or_else(|_| UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port))))
        {
            Ok(socket) => {
                if port != preferred {
                    warn!(preferred, "preferred port busy, using an ephemeral port");
                }
                return Ok(socket);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(TransportError::Bind(
        last_err.unwrap_or_else(|| std::io::Error::other("bind failed")),
    ))
}

/// Map IPv4 targets to IPv4-mapped IPv6 when the local socket is IPv6.
fn normalize(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(v4) => SocketAddr::from((v4.ip().to_ipv6_mapped(), v4.port())),
        v6 => v6,
    }
}
