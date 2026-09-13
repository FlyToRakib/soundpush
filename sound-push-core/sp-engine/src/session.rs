//! One task per connection: handshake, control I/O and media datagram dispatch.
//!
//! The session task holds no business logic. It forwards control messages to
//! the engine actor and executes the actor's commands.

use std::collections::HashMap;
use std::time::Duration;

use sp_protocol::MediaPacket;
use sp_protocol::control::{ControlMsg, Goodbye, Hello, PairRequest, StopReason, control_msg::Body};
use sp_protocol::version::{LOCAL_VERSIONS, ProtocolVersion, VersionRange};
use sp_security::DeviceId;
use sp_transport::SecureConnection;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::pipeline::receiver::PacketSink;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) enum SessionCmd {
    Send(ControlMsg),
    AddSink(u8, PacketSink),
    RemoveSink(u8),
    Close(StopReason),
}

pub(crate) struct Established {
    pub conn_id: u64,
    pub conn: SecureConnection,
    pub hello: Hello,
    pub dialed: bool,
    pub tx: mpsc::UnboundedSender<SessionCmd>,
}

pub(crate) enum SessionEvent {
    Established(Established),
    Control { conn_id: u64, msg: ControlMsg },
    Closed { conn_id: u64, reason: StopReason },
    HandshakeFailed { incompatible: bool },
}

/// Run a session over an already-authenticated QUIC connection.
pub(crate) async fn run(
    conn: SecureConnection,
    dialed: bool,
    conn_id: u64,
    local_hello: Hello,
    pair: Option<PairRequest>,
    events: mpsc::UnboundedSender<SessionEvent>,
) {
    let handshake = async {
        let (mut tx, mut rx) = if dialed {
            let (mut tx, rx) = conn.open_control().await?;
            tx.send(&ControlMsg::new(0, Body::Hello(local_hello.clone()))).await?;
            (tx, rx)
        } else {
            conn.accept_control().await?
        };
        let first = rx.recv_timeout(HANDSHAKE_TIMEOUT).await?;
        let Some(Body::Hello(peer_hello)) = first.body else {
            return Err(sp_transport::TransportError::Closed);
        };
        if !dialed {
            tx.send(&ControlMsg::new(0, Body::Hello(local_hello.clone()))).await?;
        }
        Ok::<_, sp_transport::TransportError>((tx, rx, peer_hello))
    };

    let (mut tx, mut rx, peer_hello) = match tokio::time::timeout(HANDSHAKE_TIMEOUT * 2, handshake).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            debug!(error = %e, "handshake failed");
            conn.close(1, b"handshake");
            let _ = events.send(SessionEvent::HandshakeFailed { incompatible: false });
            return;
        }
        Err(_) => {
            conn.close(1, b"timeout");
            let _ = events.send(SessionEvent::HandshakeFailed { incompatible: false });
            return;
        }
    };

    // The claimed device id must match the authenticated key.
    let claimed = DeviceId::try_from_slice(&peer_hello.device_id);
    if claimed != Some(conn.peer_fingerprint().device_id()) {
        conn.close(2, b"identity");
        let _ = events.send(SessionEvent::HandshakeFailed { incompatible: false });
        return;
    }

    let peer_range = VersionRange {
        min: ProtocolVersion::from_u32(peer_hello.protocol_min),
        max: ProtocolVersion::from_u32(peer_hello.protocol_max),
    };
    if LOCAL_VERSIONS.negotiate(peer_range).is_err() {
        let _ = tx
            .send(&ControlMsg::new(
                0,
                Body::Goodbye(Goodbye {
                    reason: StopReason::IncompatibleVersion as i32,
                }),
            ))
            .await;
        conn.close(3, b"version");
        let _ = events.send(SessionEvent::HandshakeFailed { incompatible: true });
        return;
    }

    if let Some(pair) = pair {
        if tx.send(&ControlMsg::new(0, Body::PairRequest(pair))).await.is_err() {
            let _ = events.send(SessionEvent::HandshakeFailed { incompatible: false });
            return;
        }
    }

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    info!(peer = %conn.peer_fingerprint().device_id().short(), dialed, "session established");
    if events
        .send(SessionEvent::Established(Established {
            conn_id,
            conn: conn.clone(),
            hello: peer_hello,
            dialed,
            tx: cmd_tx,
        }))
        .is_err()
    {
        conn.close(0, b"engine stopped");
        return;
    }

    let mut sinks: HashMap<u8, PacketSink> = HashMap::new();
    let reason = loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(msg) => {
                    if let Some(Body::Goodbye(g)) = &msg.body {
                        break StopReason::try_from(g.reason).unwrap_or(StopReason::PeerStopped);
                    }
                    if events.send(SessionEvent::Control { conn_id, msg }).is_err() {
                        break StopReason::UserStopped;
                    }
                }
                Err(e) => {
                    debug!(error = %e, "control stream ended");
                    break StopReason::NetworkLost;
                }
            },
            cmd = cmd_rx.recv() => match cmd {
                Some(SessionCmd::Send(msg)) => {
                    if tx.send(&msg).await.is_err() {
                        break StopReason::NetworkLost;
                    }
                }
                Some(SessionCmd::AddSink(route, sink)) => {
                    sinks.insert(route, sink);
                }
                Some(SessionCmd::RemoveSink(route)) => {
                    sinks.remove(&route);
                }
                Some(SessionCmd::Close(reason)) => {
                    let _ = tx.send(&ControlMsg::new(0, Body::Goodbye(Goodbye { reason: reason as i32 }))).await;
                    tx.finish();
                    // Give the goodbye a moment to leave before closing.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    break reason;
                }
                None => break StopReason::UserStopped,
            },
            dgram = conn.read_datagram() => match dgram {
                Ok(bytes) => {
                    if let Ok(packet) = MediaPacket::decode(bytes) {
                        if let Some(sink) = sinks.get_mut(&packet.header.route) {
                            sink.push(packet);
                        }
                    }
                }
                Err(_) => break StopReason::NetworkLost,
            },
        }
    };

    conn.close(0, b"bye");
    let _ = events.send(SessionEvent::Closed { conn_id, reason });
}

trait DeviceIdExt {
    fn try_from_slice(bytes: &[u8]) -> Option<DeviceId>;
}

impl DeviceIdExt for DeviceId {
    fn try_from_slice(bytes: &[u8]) -> Option<DeviceId> {
        bytes.try_into().ok().map(DeviceId)
    }
}
