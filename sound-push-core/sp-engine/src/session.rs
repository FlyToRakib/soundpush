//! One task per connection: handshake, control I/O and media datagram dispatch.
//!
//! The session task holds no business logic. It forwards control messages to
//! the engine actor and executes the actor's commands.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sp_protocol::MediaPacket;
use sp_protocol::control::{ControlMsg, Goodbye, Hello, PairRequest, StopReason, control_msg::Body};
use sp_protocol::probe::{self, Probe, ProbeKind};
use sp_protocol::version::{LOCAL_VERSIONS, ProtocolVersion, VersionRange};
use sp_security::DeviceId;
use sp_transport::SecureConnection;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::pipeline::receiver::PacketSink;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Control messages a peer may send per second on average, and in a burst. Legitimate traffic
/// is a few messages per second (stats, pings, route changes).
const CONTROL_RATE: f64 = 100.0;
const CONTROL_BURST: f64 = 400.0;
/// A peer that keeps flooding after this many dropped messages is disconnected.
const MAX_DROPPED_CONTROL: u32 = 1000;
/// Echoes a responder sends per second at most (probe traffic of a network test is far lower).
const ECHO_RATE: f64 = 400.0;

pub(crate) enum SessionCmd {
    Send(ControlMsg),
    AddSink(u8, PacketSink),
    RemoveSink(u8),
    /// Responder side of a network test: echo probes with this id until the deadline.
    ArmEcho {
        test_id: u32,
        until: Instant,
    },
    /// Initiator side of a network test: deliver echoes here (`None` detaches).
    ProbeSink(Option<mpsc::Sender<ProbeEcho>>),
    Close(StopReason),
}

/// A probe echo and when it arrived.
pub(crate) struct ProbeEcho {
    pub probe: Probe,
    pub received: Instant,
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
    HandshakeFailed { conn_id: u64, incompatible: bool },
}

/// Token bucket.
struct RateLimit {
    tokens: f64,
    rate: f64,
    burst: f64,
    last: Instant,
}

impl RateLimit {
    fn new(rate: f64, burst: f64) -> Self {
        Self {
            tokens: burst,
            rate,
            burst,
            last: Instant::now(),
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Run a session over an already-authenticated connection.
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
            let _ = events.send(SessionEvent::HandshakeFailed {
                conn_id,
                incompatible: false,
            });
            return;
        }
        Err(_) => {
            conn.close(1, b"timeout");
            let _ = events.send(SessionEvent::HandshakeFailed {
                conn_id,
                incompatible: false,
            });
            return;
        }
    };

    // The claimed device id must match the authenticated key.
    let claimed = DeviceId::try_from_slice(&peer_hello.device_id);
    if claimed != Some(conn.peer_fingerprint().device_id()) {
        conn.close(2, b"identity");
        let _ = events.send(SessionEvent::HandshakeFailed {
            conn_id,
            incompatible: false,
        });
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
        let _ = events.send(SessionEvent::HandshakeFailed {
            conn_id,
            incompatible: true,
        });
        return;
    }

    if let Some(pair) = pair {
        if tx.send(&ControlMsg::new(0, Body::PairRequest(pair))).await.is_err() {
            let _ = events.send(SessionEvent::HandshakeFailed {
                conn_id,
                incompatible: false,
            });
            return;
        }
    }

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    info!(peer = %conn.peer_fingerprint().device_id().short(), dialed, transport = ?conn.transport(), "session established");
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
    let mut control_limit = RateLimit::new(CONTROL_RATE, CONTROL_BURST);
    let mut dropped_control = 0u32;
    let mut echo: Option<(u32, Instant, RateLimit)> = None;
    let mut probe_sink: Option<mpsc::Sender<ProbeEcho>> = None;
    let reason = loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(msg) => {
                    if let Some(Body::Goodbye(g)) = &msg.body {
                        break StopReason::try_from(g.reason).unwrap_or(StopReason::PeerStopped);
                    }
                    // Bounded work per peer: a flood is dropped here instead of queueing in the actor.
                    if !control_limit.allow(Instant::now()) {
                        dropped_control += 1;
                        if dropped_control == 1 {
                            warn!(peer = %conn.peer_fingerprint().device_id().short(), "control message flood, dropping");
                        }
                        if dropped_control > MAX_DROPPED_CONTROL {
                            break StopReason::Unspecified;
                        }
                        continue;
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
                Some(SessionCmd::ArmEcho { test_id, until }) => {
                    echo = Some((test_id, until, RateLimit::new(ECHO_RATE, ECHO_RATE)));
                }
                Some(SessionCmd::ProbeSink(sink)) => probe_sink = sink,
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
                Ok(bytes) if probe::is_probe(&bytes) => {
                    let now = Instant::now();
                    let Ok(p) = Probe::decode(&bytes) else { continue };
                    match p.kind {
                        ProbeKind::Probe => {
                            if let Some((test_id, until, limit)) = echo.as_mut() {
                                if p.test_id == *test_id && now < *until && limit.allow(now) {
                                    // Echoes carry no padding: never more bytes out than in.
                                    let _ = conn.send_datagram(p.echo(0).encode(0));
                                }
                            }
                        }
                        ProbeKind::Echo => {
                            if let Some(sink) = &probe_sink {
                                let _ = sink.try_send(ProbeEcho { probe: p, received: now });
                            }
                        }
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_allows_bursts_then_refills() {
        let start = Instant::now();
        let mut limit = RateLimit {
            tokens: 5.0,
            rate: 10.0,
            burst: 5.0,
            last: start,
        };
        assert_eq!((0..10).filter(|_| limit.allow(start)).count(), 5);
        assert!(limit.allow(start + Duration::from_millis(100)));
        assert!(!limit.allow(start + Duration::from_millis(100)));
        // Idle time never banks more than the burst.
        assert_eq!(
            (0..20).filter(|_| limit.allow(start + Duration::from_secs(60))).count(),
            5
        );
    }
}
