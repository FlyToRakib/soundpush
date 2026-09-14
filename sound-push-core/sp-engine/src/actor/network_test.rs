//! Network self-test commands: the initiator starts a test and runs [`crate::nettest::run`];
//! the responder arms its session to echo probes. Measurement itself lives in `crate::nettest`.

use std::sync::atomic::AtomicU32;

use super::*;
use crate::nettest::{self, NetworkTestStatus, NetworkTestView, Progress, RESPONDER_WINDOW, TestPlan};

/// How long the peer has to accept a test.
const READY_TIMEOUT: Duration = Duration::from_secs(3);

enum Phase {
    AwaitingReady { deadline: Instant },
    Running(tokio::task::AbortHandle),
    Finished,
}

pub(crate) struct NetTest {
    test_id: u32,
    phase: Phase,
    started_unix: u64,
    progress: Progress,
    replies: Vec<Reply<NetworkReport>>,
    status: NetworkTestStatus,
    report: Option<NetworkReport>,
    error: Option<EngineError>,
}

impl Actor {
    pub(super) fn start_network_test(&mut self, peer: DeviceId, reply: Reply<NetworkReport>) {
        let Some(session) = self.sessions.get(&peer) else {
            let _ = reply.send(Err(if self.trust.get(&peer).is_some() {
                EngineError::Unreachable
            } else {
                EngineError::NotPaired
            }));
            return;
        };
        if !Capabilities(session.hello.capabilities).has(Capabilities::FEATURE_NETWORK_TEST) {
            let _ = reply.send(Err(EngineError::IncompatibleVersion));
            return;
        }
        if let Some(test) = self.net_tests.get_mut(&peer) {
            if test.status == NetworkTestStatus::Running {
                test.replies.push(reply);
                return;
            }
        }
        self.next_test_id = self.next_test_id.wrapping_add(1).max(1);
        let test_id = self.next_test_id;
        session.send(Body::NetTestStart(NetTestStart {
            test_id,
            duration_ms: RESPONDER_WINDOW.as_millis() as u32,
        }));
        self.net_tests.insert(
            peer,
            NetTest {
                test_id,
                phase: Phase::AwaitingReady {
                    deadline: Instant::now() + READY_TIMEOUT,
                },
                started_unix: now_unix(),
                progress: Arc::new(AtomicU32::new(0)),
                replies: vec![reply],
                status: NetworkTestStatus::Running,
                report: None,
                error: None,
            },
        );
    }

    pub(super) fn on_net_test_ready(&mut self, peer: DeviceId, ready: NetTestReady) {
        let awaiting = self
            .net_tests
            .get(&peer)
            .is_some_and(|t| t.test_id == ready.test_id && matches!(t.phase, Phase::AwaitingReady { .. }));
        if !awaiting {
            return;
        }
        if !ready.accepted {
            self.finish_network_test(peer, Err(EngineError::PeerDenied));
            return;
        }
        let (Some(session), Some(test)) = (self.sessions.get(&peer), self.net_tests.get_mut(&peer)) else {
            return;
        };
        let (tx, rx) = mpsc::channel(4096);
        let _ = session.tx.send(SessionCmd::ProbeSink(Some(tx)));
        let conn = session.conn.clone();
        let transport = match conn.transport() {
            TransportKind::Quic => "quic",
            TransportKind::Tcp => "tcp",
        };
        let max_datagram = conn.max_datagram_size().unwrap_or(0) as u32;
        let (test_id, progress) = (test.test_id, test.progress.clone());
        let internal = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            let report = nettest::run(
                Arc::new(conn),
                transport,
                max_datagram,
                test_id,
                rx,
                progress,
                TestPlan::default(),
            )
            .await;
            let _ = internal.send(Internal::NetTestFinished { peer, test_id, report });
        });
        test.phase = Phase::Running(task.abort_handle());
    }

    pub(super) fn on_net_test_finished(&mut self, peer: DeviceId, test_id: u32, report: NetworkReport) {
        if self
            .net_tests
            .get(&peer)
            .is_some_and(|t| t.test_id == test_id && matches!(t.phase, Phase::Running(_)))
        {
            self.finish_network_test(peer, Ok(report));
        }
    }

    /// Responder: echo this test's probes for a bounded time. Only trusted, connected peers get
    /// here, and echoes are rate-limited and unpadded in the session task.
    pub(super) fn on_net_test_start(&mut self, peer: DeviceId, start: NetTestStart) {
        let Some(session) = self.sessions.get(&peer) else {
            return;
        };
        let window = Duration::from_millis(start.duration_ms.into()).min(RESPONDER_WINDOW);
        let _ = session.tx.send(SessionCmd::ArmEcho {
            test_id: start.test_id,
            until: Instant::now() + window,
        });
        session.send(Body::NetTestReady(NetTestReady {
            test_id: start.test_id,
            accepted: true,
        }));
    }

    pub(super) fn on_net_test_stop(&mut self, peer: DeviceId, stop: NetTestStop) {
        if let Some(session) = self.sessions.get(&peer) {
            let _ = session.tx.send(SessionCmd::ArmEcho {
                test_id: stop.test_id,
                until: Instant::now(),
            });
        }
    }

    pub(super) fn cancel_network_test(&mut self, peer: DeviceId) {
        self.end_network_test(peer, EngineError::Cancelled);
    }

    /// Stop a running test for `peer` (disconnect, revocation, cancel).
    pub(super) fn end_network_test(&mut self, peer: DeviceId, error: EngineError) {
        if self
            .net_tests
            .get(&peer)
            .is_some_and(|t| t.status == NetworkTestStatus::Running)
        {
            self.finish_network_test(peer, Err(error));
        }
    }

    fn finish_network_test(&mut self, peer: DeviceId, result: Result<NetworkReport, EngineError>) {
        let Some(test) = self.net_tests.get_mut(&peer) else {
            return;
        };
        if let Phase::Running(task) = &test.phase {
            task.abort();
        }
        test.phase = Phase::Finished;
        for reply in test.replies.drain(..) {
            let _ = reply.send(result.clone());
        }
        match result {
            Ok(report) => {
                test.status = NetworkTestStatus::Done;
                test.report = Some(report);
                test.error = None;
            }
            Err(error) => {
                test.status = if error == EngineError::Cancelled {
                    NetworkTestStatus::Cancelled
                } else {
                    NetworkTestStatus::Failed
                };
                test.error = Some(error);
            }
        }
        let test_id = test.test_id;
        if let Some(s) = self.sessions.get(&peer) {
            let _ = s.tx.send(SessionCmd::ProbeSink(None));
            s.send(Body::NetTestStop(NetTestStop { test_id }));
        }
    }

    pub(super) fn tick_network_tests(&mut self, now: Instant) {
        let expired: Vec<DeviceId> = self
            .net_tests
            .iter()
            .filter(|(_, t)| matches!(t.phase, Phase::AwaitingReady { deadline } if now > deadline))
            .map(|(id, _)| *id)
            .collect();
        for peer in expired {
            self.finish_network_test(peer, Err(EngineError::Unreachable));
        }
    }

    pub(super) fn forget_network_test(&mut self, peer: &DeviceId) {
        self.end_network_test(*peer, EngineError::Revoked);
        self.net_tests.remove(peer);
    }

    pub(super) fn abort_network_tests(&mut self) {
        for test in self.net_tests.values() {
            if let Phase::Running(task) = &test.phase {
                task.abort();
            }
        }
        self.net_tests.clear();
    }

    pub(super) fn network_test_views(&self) -> Vec<NetworkTestView> {
        let mut views: Vec<NetworkTestView> = self
            .net_tests
            .iter()
            .map(|(peer, t)| NetworkTestView {
                peer_id: peer.to_hex(),
                status: t.status,
                progress: if t.status == NetworkTestStatus::Done {
                    1.0
                } else {
                    t.progress.load(Ordering::Relaxed) as f32 / 1000.0
                },
                report: t.report.clone(),
                error: t.error.as_ref().map(ErrorView::from),
                started_unix: t.started_unix,
            })
            .collect();
        views.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
        views
    }
}
