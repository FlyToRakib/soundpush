//! Discovery service: runs mDNS + beacons and emits merged events.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};
use sp_protocol::Capabilities;
use sp_security::DeviceIdentity;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tracing::{debug, warn};

use crate::beacon::{Beacon, FLAG_NAME_HIDDEN};
use crate::mdns::{MdnsAdvertiser, MdnsRecord};
use crate::registry::{DiscoveryEvent, PeerRegistry};
use crate::{BEACON_PORT, PeerAdvert, Source};

/// Who may see this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Name and presence advertised to everyone on the network.
    Everyone,
    /// Presence advertised without a name (trusted peers recognize the device ID).
    TrustedOnly,
    /// Nothing advertised; this device can still dial out.
    Hidden,
}

#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub name: String,
    pub platform: String,
    pub port: u16,
    pub capabilities: Capabilities,
    pub protocol_max: u32,
    pub visibility: Visibility,
    pub beacon_interval: Duration,
    /// Browse for peers (mobile disables this in background to save battery).
    pub browse: bool,
}

enum Command {
    Update(DiscoveryConfig),
    Ingest(PeerAdvert),
}

/// Handle to the running discovery service. Dropping it stops discovery.
pub struct Discovery {
    commands: mpsc::UnboundedSender<Command>,
    _shutdown: watch::Sender<bool>,
}

impl Discovery {
    pub fn start(
        identity: Arc<DeviceIdentity>,
        config: DiscoveryConfig,
    ) -> (Self, mpsc::UnboundedReceiver<DiscoveryEvent>) {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        tokio::spawn(run(identity, config, cmd_rx, events_tx, shutdown_rx));
        (
            Self {
                commands: cmd_tx,
                _shutdown: shutdown_tx,
            },
            events_rx,
        )
    }

    /// Apply new settings (name, visibility, port…).
    pub fn update(&self, config: DiscoveryConfig) {
        let _ = self.commands.send(Command::Update(config));
    }

    /// Feed an advert from an external source (Android NSD, QR, manual entry).
    pub fn ingest(&self, advert: PeerAdvert) {
        let _ = self.commands.send(Command::Ingest(advert));
    }
}

async fn run(
    identity: Arc<DeviceIdentity>,
    mut config: DiscoveryConfig,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<DiscoveryEvent>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut registry = PeerRegistry::new(identity.device_id());
    let (found_tx, mut found_rx) = mpsc::unbounded_channel::<PeerAdvert>();
    let (lost_tx, mut lost_rx) = mpsc::unbounded_channel();

    let mut mdns = match MdnsAdvertiser::new() {
        Ok(m) => Some(m),
        Err(e) => {
            warn!(error = %e, "mDNS unavailable, relying on beacons");
            None
        }
    };
    if let Some(m) = mdns.as_mut() {
        publish(m, &identity, &config);
        if config.browse {
            if let Err(e) = m.browse(found_tx.clone(), lost_tx.clone()) {
                warn!(error = %e, "mDNS browse failed");
            }
        }
    }

    let socket = match beacon_socket() {
        Ok(s) => Some(Arc::new(s)),
        Err(e) => {
            warn!(error = %e, "beacon socket unavailable");
            None
        }
    };
    if let (Some(sock), true) = (socket.clone(), config.browse) {
        let found = found_tx.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            while let Ok((n, from)) = sock.recv_from(&mut buf).await {
                match Beacon::decode(&buf[..n]) {
                    Ok(b) => {
                        let advert = PeerAdvert {
                            device_id: b.fingerprint().device_id(),
                            public_key: Some(b.public_key),
                            name: b.name,
                            platform: b.platform,
                            addresses: vec![SocketAddr::new(from.ip(), b.port)],
                            capabilities: b.capabilities,
                            protocol_max: b.protocol_max,
                            source: Source::Beacon,
                        };
                        if found.send(advert).is_err() {
                            break;
                        }
                    }
                    Err(e) => debug!(%from, error = %e, "ignored beacon"),
                }
            }
        });
    }

    let mut beacon_tick = tokio::time::interval(config.beacon_interval);
    let mut sweep_tick = tokio::time::interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            Some(cmd) = commands.recv() => match cmd {
                Command::Update(new) => {
                    config = new;
                    if let Some(m) = mdns.as_mut() {
                        publish(m, &identity, &config);
                    }
                    beacon_tick = tokio::time::interval(config.beacon_interval);
                }
                Command::Ingest(advert) => {
                    if let Some(ev) = registry.ingest(advert, Instant::now()) {
                        let _ = events.send(ev);
                    }
                }
            },
            Some(advert) = found_rx.recv() => {
                if let Some(ev) = registry.ingest(advert, Instant::now()) {
                    let _ = events.send(ev);
                }
            }
            Some(id) = lost_rx.recv() => {
                if let Some(ev) = registry.remove(&id) {
                    let _ = events.send(ev);
                }
            }
            _ = beacon_tick.tick() => {
                if let (Some(sock), false) = (socket.as_ref(), config.visibility == Visibility::Hidden) {
                    send_beacon(sock, &identity, &config).await;
                }
            }
            _ = sweep_tick.tick() => {
                for ev in registry.sweep(Instant::now()) {
                    let _ = events.send(ev);
                }
            }
        }
    }
}

fn publish(mdns: &mut MdnsAdvertiser, identity: &DeviceIdentity, config: &DiscoveryConfig) {
    if config.visibility == Visibility::Hidden {
        mdns.unpublish();
        return;
    }
    let name = (config.visibility == Visibility::Everyone).then_some(config.name.as_str());
    let record = MdnsRecord {
        device_id: identity.device_id(),
        name,
        platform: &config.platform,
        port: config.port,
        capabilities: config.capabilities,
        protocol_max: config.protocol_max,
    };
    if let Err(e) = mdns.publish(&record) {
        warn!(error = %e, "mDNS publish failed");
    }
}

async fn send_beacon(socket: &UdpSocket, identity: &DeviceIdentity, config: &DiscoveryConfig) {
    let flags = if config.visibility == Visibility::Everyone { 0 } else { FLAG_NAME_HIDDEN };
    let beacon = Beacon {
        public_key: identity.public_key(),
        port: config.port,
        capabilities: config.capabilities,
        protocol_max: config.protocol_max,
        flags,
        platform: config.platform.clone(),
        name: config.name.clone(),
    };
    let wire = beacon.encode(identity);
    let target = SocketAddr::from((Ipv4Addr::BROADCAST, BEACON_PORT));
    if let Err(e) = socket.send_to(&wire, target).await {
        debug!(error = %e, "beacon send failed");
    }
}

fn beacon_socket() -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(all(unix, not(target_os = "solaris")))]
    socket.set_reuse_port(true)?;
    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, BEACON_PORT)).into())?;
    UdpSocket::from_std(socket.into())
}
