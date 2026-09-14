//! QUIC endpoint: listens for peers and dials them.

use std::net::{Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use sp_security::{DeviceId, DeviceIdentity};
use tracing::{debug, warn};

use crate::connection::SecureConnection;
use crate::tls::{Credentials, SERVER_NAME};
use crate::{ALPN, TransportError};

/// Preferred listening port (UDP). Falls back to an ephemeral port when busy.
pub const DEFAULT_PORT: u16 = 47650;

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub preferred_port: u16,
    pub keep_alive: Duration,
    pub idle_timeout: Duration,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            preferred_port: DEFAULT_PORT,
            keep_alive: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(10),
        }
    }
}

pub struct Endpoint {
    inner: quinn::Endpoint,
    credentials: Credentials,
    transport: Arc<quinn::TransportConfig>,
    /// True when bound to a dual-stack IPv6 socket (IPv4 targets are then IPv4-mapped).
    ipv6: bool,
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
        let quic_client = quinn::crypto::rustls::QuicClientConfig::try_from(
            self.credentials.client_config(pinned, ALPN)?,
        )
        .map_err(|e| TransportError::Tls(e.to_string()))?;
        let mut client_config = quinn::ClientConfig::new(Arc::new(quic_client));
        client_config.transport_config(self.transport.clone());

        let addr = if self.ipv6 { normalize(addr) } else { addr };
        debug!(%addr, "dialing peer");
        let connecting = self.inner.connect_with(client_config, addr, SERVER_NAME)?;
        let conn = connecting.await?;
        SecureConnection::from_quic(conn)
    }

    /// Wait for the next connection attempt. Returns `None` when the endpoint is closed.
    /// The handshake is completed by [`Handshake::finish`], so a caller can run several at
    /// once and one stalled peer does not hold up the others.
    pub async fn accept(&self) -> Option<Handshake> {
        self.inner
            .accept()
            .await
            .map(|incoming| Handshake { incoming })
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
}

impl Handshake {
    pub fn remote_address(&self) -> SocketAddr {
        self.incoming.remote_address()
    }

    /// Complete the handshake (TLS with mutual authentication).
    pub async fn finish(self) -> Result<SecureConnection, TransportError> {
        let remote = self.incoming.remote_address();
        match self.incoming.await {
            Ok(conn) => SecureConnection::from_quic(conn),
            Err(e) => {
                warn!(%remote, error = %e, "incoming handshake failed");
                Err(e.into())
            }
        }
    }
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
