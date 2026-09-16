//! Engines in one process over real QUIC and TCP on loopback: pairing, routes, audio, stop,
//! reconnect and resume, shared encoders, per-device profiles, USB (TCP) and the network test.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers fail the test on purpose

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sp_security::pairing::QrPairingPayload;

use sp_engine::settings::{DeviceProfile, QualityMode};
use sp_engine::sp_audio_io::null::NullBackend;
use sp_engine::sp_audio_io::{
    AudioBackend, AudioError, AudioStream, CaptureCallback, CaptureSource, DeviceInfo,
    ErrorCallback, RenderCallback, RenderTarget,
};
use sp_engine::state::RouteStatus;
use sp_engine::{AuditKind, EngineConfig, EngineHandle, EngineState, PlatformHooks, RouteKind};

struct TestHooks {
    dir: PathBuf,
    name: &'static str,
    platform: &'static str,
    backend: Arc<dyn AudioBackend>,
    /// Every `set_speakers_muted` call, in order.
    speakers: Arc<Mutex<Vec<bool>>>,
}

impl PlatformHooks for TestHooks {
    fn set_speakers_muted(&self, muted: bool) -> bool {
        self.speakers.lock().unwrap().push(muted);
        true
    }
    fn data_dir(&self) -> PathBuf {
        self.dir.clone()
    }
    fn storage_key(&self) -> [u8; 32] {
        [42; 32]
    }
    fn platform(&self) -> &'static str {
        self.platform
    }
    fn default_device_name(&self) -> String {
        self.name.to_string()
    }
    fn audio_backend(&self) -> Arc<dyn AudioBackend> {
        self.backend.clone()
    }
}

/// A null backend that counts opened capture streams.
struct Counting {
    inner: NullBackend,
    captures: Arc<AtomicUsize>,
}

impl AudioBackend for Counting {
    fn name(&self) -> &'static str {
        "counting"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        self.inner.list_devices()
    }
    fn supports_loopback(&self) -> bool {
        true
    }
    fn open_capture(
        &self,
        source: &CaptureSource,
        channels: u16,
        on_audio: CaptureCallback,
        on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        self.captures.fetch_add(1, Ordering::Relaxed);
        self.inner
            .open_capture(source, channels, on_audio, on_error)
    }
    fn open_render(
        &self,
        target: &RenderTarget,
        channels: u16,
        on_audio: RenderCallback,
        on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        self.inner.open_render(target, channels, on_audio, on_error)
    }
}

fn config() -> EngineConfig {
    EngineConfig {
        app_version: "test".into(),
        port: 0,
        discovery: false,
        include_loopback: true,
        tcp_listener: true,
    }
}

fn sine() -> Arc<NullBackend> {
    Arc::new(NullBackend {
        recorded: None,
        capture_frequency: 440.0,
    })
}

fn recorder() -> (Arc<NullBackend>, Arc<Mutex<Vec<f32>>>) {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    (
        Arc::new(NullBackend {
            recorded: Some(recorded.clone()),
            capture_frequency: 0.0,
        }),
        recorded,
    )
}

fn start_with(
    dir: &tempfile::TempDir,
    name: &'static str,
    platform: &'static str,
    backend: Arc<dyn AudioBackend>,
    config: EngineConfig,
) -> EngineHandle {
    EngineHandle::start(
        Arc::new(TestHooks {
            dir: dir.path().to_path_buf(),
            name,
            platform,
            backend,
            speakers: Arc::default(),
        }),
        config,
    )
    .expect("engine starts")
}

/// Like `start`, returning the engine's record of speaker mutes.
fn start_recording_speakers(
    dir: &tempfile::TempDir,
    name: &'static str,
) -> (EngineHandle, Arc<Mutex<Vec<bool>>>) {
    let speakers = Arc::new(Mutex::new(Vec::new()));
    let engine = EngineHandle::start(
        Arc::new(TestHooks {
            dir: dir.path().to_path_buf(),
            name,
            platform: "windows",
            backend: Arc::new(NullBackend::default()),
            speakers: speakers.clone(),
        }),
        config(),
    )
    .expect("engine starts");
    (engine, speakers)
}

#[test]
fn peer_speaker_mute_is_shown_and_ends_with_the_session() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let (desk, speakers) = start_recording_speakers(&desk_dir, "Desk");
    let phone = start(&phone_dir, "Phone", Arc::new(NullBackend::default()));
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (desk_id, phone_id) = pair(&rt, &desk, &phone);
    desk.set_permission(
        phone_id.clone(),
        sp_engine::PermissionKind::ControlMe,
        sp_engine::Policy::Allow,
    )
    .unwrap();

    // "Mute PC" on the phone targets the desk, and the phone's state shows it.
    phone
        .set_peer_speakers_muted(desk_id.clone(), true)
        .unwrap();
    wait_for(&phone, "phone shows the desk muted", |s| {
        s.peers
            .iter()
            .any(|p| p.device_id == desk_id && p.speakers_muted && !p.remote_address.is_empty())
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while speakers.lock().unwrap().last() != Some(&true) {
        assert!(Instant::now() < deadline, "desk speakers were not muted");
        std::thread::sleep(Duration::from_millis(50));
    }

    // The phone disappears: the desk must not stay muted.
    desk.simulate_connection_loss(phone_id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while speakers.lock().unwrap().last() != Some(&false) {
        assert!(Instant::now() < deadline, "desk speakers stayed muted");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn start(
    dir: &tempfile::TempDir,
    name: &'static str,
    backend: Arc<dyn AudioBackend>,
) -> EngineHandle {
    start_with(dir, name, "linux", backend, config())
}

fn wait_for(
    engine: &EngineHandle,
    what: &str,
    pred: impl Fn(&EngineState) -> bool,
) -> Arc<EngineState> {
    wait_for_within(engine, what, Duration::from_secs(10), pred)
}

fn wait_for_within(
    engine: &EngineHandle,
    what: &str,
    timeout: Duration,
    pred: impl Fn(&EngineState) -> bool,
) -> Arc<EngineState> {
    let deadline = Instant::now() + timeout;
    loop {
        let state = engine.state();
        if pred(&state) {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {state:#?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn connected(s: &EngineState, peer: &str) -> bool {
    s.peers
        .iter()
        .any(|p| p.device_id == peer && p.trusted && p.connection.is_connected())
}

/// `a` shows a QR code and `b` scans it; returns (a id, b id) once both are connected.
fn pair(rt: &tokio::runtime::Runtime, a: &EngineHandle, b: &EngineHandle) -> (String, String) {
    let uri = rt.block_on(a.start_pairing()).unwrap();
    rt.block_on(b.pair_with_qr(uri)).unwrap();
    let a_id = a.state().local.device_id.clone();
    let b_id = b.state().local.device_id.clone();
    wait_for(a, "a trusts b", |s| connected(s, &b_id));
    wait_for(b, "b trusts a", |s| connected(s, &a_id));
    (a_id, b_id)
}

fn peak_of_last_second(recorded: &Mutex<Vec<f32>>) -> f32 {
    let audio = recorded.lock().unwrap();
    audio[audio.len().saturating_sub(48_000)..]
        .iter()
        .fold(0f32, |m, s| m.max(s.abs()))
}

fn wait_for_audio(recorded: &Mutex<Vec<f32>>, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if peak_of_last_second(recorded) > 0.1 {
            return;
        }
        assert!(Instant::now() < deadline, "no audio: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn qr_pairing_route_and_audio() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let (phone_backend, recorded) = recorder();
    let desk = start(&desk_dir, "Desk", sine());
    let phone = start(&phone_dir, "Phone", phone_backend);
    let rt = tokio::runtime::Runtime::new().unwrap();

    let (desk_id, phone_id) = pair(&rt, &desk, &phone);
    assert!(
        desk.state().pairing.qr_uri.is_none(),
        "QR secret is single-use"
    );

    // Phone asks to hear the desk's system audio.
    let route_id = rt
        .block_on(phone.start_route(desk_id.clone(), RouteKind::ReceiveSystemAudio))
        .unwrap();
    wait_for(&desk, "desk route active", |s| {
        s.routes
            .iter()
            .any(|r| r.kind == RouteKind::SendSystemAudio && r.status == RouteStatus::Active)
    });
    wait_for_audio(&recorded, "phone should play the desk's sine");

    // Snapshots are throttled, so the first one after audio starts may predate the statistics.
    let phone_state = wait_for(&phone, "route statistics", |s| {
        s.routes
            .iter()
            .any(|r| r.route_id == route_id && r.stats.latency_ms > 0.0)
    });
    let route = phone_state
        .routes
        .iter()
        .find(|r| r.route_id == route_id)
        .unwrap();
    assert_eq!(route.stats.codec, "Opus");
    assert!(
        phone_state
            .peers
            .iter()
            .any(|p| p.device_id == desk_id && p.transport == "quic")
    );

    // Microphone permission defaults to "Ask": desk asks phone for its mic.
    let pending = std::thread::spawn({
        let desk = desk.clone();
        let phone_id = phone_id.clone();
        move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(desk.start_route(phone_id, RouteKind::ReceiveMicToSpeaker))
        }
    });
    let prompt = wait_for(&phone, "mic permission prompt", |s| !s.requests.is_empty());
    phone
        .respond_route_request(prompt.requests[0].request_id, true, false)
        .unwrap();
    assert!(pending.join().unwrap().is_ok());

    // Stopping on one side stops on both.
    phone.stop_route(route_id).unwrap();
    wait_for(&desk, "desk system route stopped", |s| {
        !s.routes
            .iter()
            .any(|r| r.kind == RouteKind::SendSystemAudio)
    });

    // The phone's security log saw the pairing, the approval and the stream (plan §21).
    let log = rt.block_on(phone.audit_log()).unwrap();
    for kind in [
        AuditKind::PairingSucceeded,
        AuditKind::RouteApproved,
        AuditKind::RouteStarted,
        AuditKind::RouteStopped,
    ] {
        assert!(log.iter().any(|e| e.kind == kind), "{kind:?} in {log:#?}");
    }
    assert!(
        log.windows(2).all(|w| w[0].time_unix >= w[1].time_unix),
        "newest first"
    );

    // Forgetting a device revokes access.
    desk.forget_device(phone_id.clone()).unwrap();
    wait_for(&desk, "phone forgotten", |s| {
        !s.peers.iter().any(|p| p.device_id == phone_id && p.trusted)
    });
    let log = rt.block_on(desk.audit_log()).unwrap();
    assert_eq!(log[0].kind, AuditKind::DeviceForgotten, "{log:#?}");
    rt.block_on(desk.clear_audit_log()).unwrap();
    let log = rt.block_on(desk.audit_log()).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].kind, AuditKind::LogCleared);
}

#[test]
fn unreachable_candidates_do_not_delay_the_connection() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let desk = start(&desk_dir, "Desk", sine());
    let phone = start(&phone_dir, "Phone", Arc::new(NullBackend::default()));
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Put two black holes ahead of the desk's best address in its pairing code (a code carries
    // three). They rank best (192.168/16) and equal ranks keep their order, so they are dialled
    // first; each one costs the full 3 s handshake timeout. Dialling in order would need about
    // 6 s; racing the candidates 250 ms apart reaches the real address at once (plan §17.3).
    let uri = rt.block_on(desk.start_pairing()).unwrap();
    let mut payload = QrPairingPayload::from_uri(&uri).unwrap();
    let port = desk.state().local.port;
    payload.addresses.truncate(1);
    for octet in [11u8, 12] {
        payload
            .addresses
            .insert(0, SocketAddr::from(([192, 168, 254, octet], port)));
    }
    let desk_id = desk.state().local.device_id.clone();
    let started = Instant::now();
    rt.block_on(phone.pair_with_qr(payload.to_uri())).unwrap();
    wait_for_within(
        &phone,
        "phone connects past the black holes",
        Duration::from_secs(4),
        |s| connected(s, &desk_id),
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "candidates were not raced: {:?}",
        started.elapsed()
    );
}

#[test]
fn pairing_attempts_are_rate_limited_per_address() {
    let desk_dir = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let desk = start(&desk_dir, "Desk", sine());
    let other = start(&other_dir, "Stranger", sine());
    let rt = tokio::runtime::Runtime::new().unwrap();
    let address = format!("127.0.0.1:{}", desk.state().local.port);
    let rate_limited = |s: &EngineState| {
        s.notices.iter().any(|n| {
            n.error
                .as_ref()
                .is_some_and(|e| e.key == "error.security.pairingRateLimited")
        })
    };

    // Pairing is not open on the desk: the first five attempts are refused as usual, the sixth
    // within the minute is refused for the rate limit before any pairing work.
    for i in 0..6 {
        rt.block_on(other.pair_with_address(address.clone()))
            .unwrap();
        if i < 5 {
            wait_for(&other, &format!("attempt {i} refused"), |s| {
                s.notices
                    .iter()
                    .filter(|n| n.key == "error.security.pairingRejected")
                    .count()
                    > i
            });
        }
    }
    wait_for(&other, "rate limit reported to the dialer", rate_limited);
    wait_for(&desk, "rate limit notice on the desk", |s| {
        s.notices
            .iter()
            .any(|n| n.key == "notice.pairingRateLimited")
    });
    let log = rt.block_on(desk.audit_log()).unwrap();
    assert!(
        log.iter()
            .any(|e| e.kind == AuditKind::PairingRateLimited && e.detail == "127.0.0.1"),
        "{log:#?}"
    );
    assert_eq!(
        log.iter()
            .filter(|e| e.kind == AuditKind::PairingAttempt)
            .count(),
        5
    );
}

#[test]
fn trusted_devices_reconnect_after_restart() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let backend = || Arc::new(NullBackend::default());

    let a = start(&a_dir, "A", backend());
    let b = start(&b_dir, "B", backend());
    let (a_id, b_id) = pair(&rt, &a, &b);

    // Restart A on a new port; B only knows the old address, so A must reach B.
    drop(a);
    std::thread::sleep(Duration::from_millis(300));
    let a = start(&a_dir, "A", backend());
    assert_eq!(
        a.state().local.device_id,
        a_id,
        "identity persists across restarts"
    );
    wait_for(&a, "A reconnects to B", |s| connected(s, &b_id));
}

#[test]
fn lost_connection_resumes_routes_without_asking_again() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let (desk_backend, recorded) = recorder();
    let desk = start(&desk_dir, "Desk", desk_backend);
    let phone = start(&phone_dir, "Phone", sine());
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (desk_id, phone_id) = pair(&rt, &desk, &phone);

    // The desk uses the phone's microphone; the phone's user approves once ("Ask").
    let pending = std::thread::spawn({
        let desk = desk.clone();
        let phone_id = phone_id.clone();
        move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(desk.start_route(phone_id, RouteKind::ReceiveMicToSpeaker))
        }
    });
    let prompt = wait_for(&phone, "first prompt", |s| !s.requests.is_empty());
    phone
        .respond_route_request(prompt.requests[0].request_id, true, false)
        .unwrap();
    assert!(pending.join().unwrap().is_ok());
    wait_for_audio(&recorded, "mic audio before the loss");

    // The network drops: no goodbye, both sides see the connection vanish.
    phone.simulate_connection_loss(desk_id.clone()).unwrap();
    wait_for(&phone, "phone notices the loss", |s| {
        !connected(s, &desk_id)
    });

    // Both reconnect; the resume token lets the phone restore the approved route silently.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (d, p) = (desk.state(), phone.state());
        assert!(
            p.requests.is_empty(),
            "a resumed session must not prompt again"
        );
        let active = d
            .routes
            .iter()
            .any(|r| r.kind == RouteKind::ReceiveMicToSpeaker && r.status == RouteStatus::Active)
            && p.routes
                .iter()
                .any(|r| r.kind == RouteKind::SendMicToSpeaker && r.status == RouteStatus::Active);
        if active && connected(&d, &phone_id) && connected(&p, &desk_id) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "route did not resume: {d:#?}\n{p:#?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    recorded.lock().unwrap().clear();
    wait_for_audio(&recorded, "mic audio after resuming");
}

#[test]
fn one_capture_serves_two_receivers() {
    let dirs: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    let captures = Arc::new(AtomicUsize::new(0));
    let desk = start(
        &dirs[0],
        "Desk",
        Arc::new(Counting {
            inner: NullBackend {
                recorded: None,
                capture_frequency: 440.0,
            },
            captures: captures.clone(),
        }),
    );
    let (b1, rec1) = recorder();
    let (b2, rec2) = recorder();
    let phone1 = start(&dirs[1], "Phone 1", b1);
    let phone2 = start(&dirs[2], "Phone 2", b2);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (desk_id, _) = pair(&rt, &desk, &phone1);
    pair(&rt, &desk, &phone2);

    let r1 = rt
        .block_on(phone1.start_route(desk_id.clone(), RouteKind::ReceiveSystemAudio))
        .unwrap();
    let r2 = rt
        .block_on(phone2.start_route(desk_id.clone(), RouteKind::ReceiveSystemAudio))
        .unwrap();
    wait_for(&desk, "two active send routes", |s| {
        s.routes
            .iter()
            .filter(|r| r.kind == RouteKind::SendSystemAudio && r.status == RouteStatus::Active)
            .count()
            == 2
    });
    wait_for_audio(&rec1, "phone 1 plays");
    wait_for_audio(&rec2, "phone 2 plays");
    assert_eq!(
        captures.load(Ordering::Relaxed),
        1,
        "both routes share one capture and encoder"
    );

    // One receiver leaves: the other keeps playing from the same encoder.
    phone1.stop_route(r1).unwrap();
    wait_for(&desk, "one route left", |s| {
        s.routes
            .iter()
            .filter(|r| r.kind == RouteKind::SendSystemAudio)
            .count()
            == 1
    });
    rec2.lock().unwrap().clear();
    wait_for_audio(&rec2, "phone 2 still plays");
    assert_eq!(captures.load(Ordering::Relaxed), 1);

    // The last receiver leaves: the group closes, so a new route opens a fresh capture.
    phone2.stop_route(r2).unwrap();
    wait_for(&desk, "no routes", |s| s.routes.is_empty());
    std::thread::sleep(Duration::from_millis(300));
    rt.block_on(phone1.start_route(desk_id, RouteKind::ReceiveSystemAudio))
        .unwrap();
    rec1.lock().unwrap().clear();
    wait_for_audio(&rec1, "phone 1 plays again");
    assert_eq!(
        captures.load(Ordering::Relaxed),
        2,
        "the idle encoder was released"
    );
}

#[test]
fn device_profile_switches_codec_on_a_running_route() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let (phone_backend, recorded) = recorder();
    let desk = start(&desk_dir, "Desk", sine());
    let phone = start(&phone_dir, "Phone", phone_backend);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (desk_id, _) = pair(&rt, &desk, &phone);

    let route_id = rt
        .block_on(phone.start_route(desk_id.clone(), RouteKind::ReceiveSystemAudio))
        .unwrap();
    wait_for_audio(&recorded, "opus audio");

    // The phone started the route, so its profile for the desk chooses the codec.
    phone
        .set_device_profile(
            desk_id.clone(),
            Some(DeviceProfile {
                quality: Some(QualityMode::Lossless),
                ..DeviceProfile::default()
            }),
        )
        .unwrap();
    wait_for(&phone, "phone on PCM", |s| {
        s.routes
            .iter()
            .any(|r| r.route_id == route_id && r.stats.codec == "PCM")
    });
    wait_for(&desk, "desk sends PCM", |s| {
        s.routes
            .iter()
            .any(|r| r.kind == RouteKind::SendSystemAudio && r.stats.codec == "PCM")
    });
    assert!(
        phone
            .state()
            .settings
            .device_profiles
            .contains_key(&desk_id)
    );
    recorded.lock().unwrap().clear();
    wait_for_audio(&recorded, "pcm audio after the switch");

    // Clearing the profile goes back to the global setting (automatic quality, Opus).
    phone.set_device_profile(desk_id.clone(), None).unwrap();
    wait_for(&phone, "phone back on Opus", |s| {
        s.routes
            .iter()
            .any(|r| r.route_id == route_id && r.stats.codec == "Opus")
    });
}

#[test]
fn usb_tcp_transport_pairs_streams_and_measures() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let (phone_backend, recorded) = recorder();
    let desk = start(&desk_dir, "Desk", sine());
    // A phone without network candidates (loopback QUIC addresses are not usable for it), so only
    // the USB candidate, a TCP connection to its loopback, can reach the desk.
    let phone = start_with(
        &phone_dir,
        "Phone",
        "android",
        phone_backend,
        EngineConfig {
            include_loopback: false,
            tcp_listener: false,
            ..config()
        },
    );
    let tcp_port = desk.state().local.tcp_port;
    assert_ne!(tcp_port, 0, "desk listens for USB connections");
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Code pairing through the forward, as after `adb reverse`: the phone enters its loopback
    // address, so the TCP candidate is the only path, and both users compare the TLS-derived code.
    rt.block_on(desk.start_pairing()).unwrap();
    rt.block_on(phone.pair_with_address(format!("127.0.0.1:{tcp_port}")))
        .unwrap();
    let desk_id = desk.state().local.device_id.clone();
    let phone_id = phone.state().local.device_id.clone();
    let on_desk = wait_for(&desk, "code on desk", |s| {
        s.pairing.prompts.iter().any(|p| p.peer_id == phone_id)
    });
    let on_phone = wait_for(&phone, "code on phone", |s| {
        s.pairing.prompts.iter().any(|p| p.peer_id == desk_id)
    });
    assert_eq!(
        on_desk.pairing.prompts[0].code, on_phone.pairing.prompts[0].code,
        "same code on both"
    );
    desk.confirm_pairing(phone_id.clone(), true).unwrap();
    phone.confirm_pairing(desk_id.clone(), true).unwrap();
    wait_for(&desk, "desk trusts phone", |s| connected(s, &phone_id));
    wait_for(&phone, "phone trusts desk", |s| connected(s, &desk_id));
    assert!(
        phone
            .state()
            .peers
            .iter()
            .any(|p| p.device_id == desk_id && p.transport == "tcp"),
        "connected over TCP"
    );

    rt.block_on(phone.start_route(desk_id.clone(), RouteKind::ReceiveSystemAudio))
        .unwrap();
    wait_for_audio(&recorded, "audio over TCP");

    let report = rt
        .block_on(phone.run_network_test(desk_id.clone()))
        .unwrap();
    assert_eq!(report.transport, "tcp");
    assert!(report.probes_received > 0);
    assert!(report.loss_pct < 5.0, "loopback loss {}", report.loss_pct);
    assert!(
        report.achievable_kbps >= 320,
        "achievable {}",
        report.achievable_kbps
    );
    let state = wait_for(&phone, "test result in state", |s| {
        s.network_tests
            .iter()
            .any(|t| t.peer_id == desk_id && t.report.is_some())
    });
    assert_eq!(state.network_tests[0].progress, 1.0);

    // After a lost connection the phone reconnects over USB again.
    phone.simulate_connection_loss(desk_id.clone()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    wait_for_within(&phone, "reconnect over TCP", Duration::from_secs(20), |s| {
        s.peers
            .iter()
            .any(|p| p.device_id == desk_id && p.connection.is_connected() && p.transport == "tcp")
    });
}

#[test]
fn routes_resume_after_restart_when_enabled() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let desk = start(&desk_dir, "Desk", sine());
    let phone = start(&phone_dir, "Phone", Arc::new(NullBackend::default()));
    let (desk_id, _) = pair(&rt, &desk, &phone);

    let mut settings = phone.state().settings.clone();
    settings.resume_routes_on_start = true;
    rt.block_on(phone.update_settings(settings)).unwrap();
    rt.block_on(phone.start_route(desk_id.clone(), RouteKind::ReceiveSystemAudio))
        .unwrap();
    wait_for(&phone, "route saved for restart", |s| {
        s.settings
            .saved_routes
            .iter()
            .any(|r| r.peer_id == desk_id && !r.keep)
    });

    drop(phone);
    std::thread::sleep(Duration::from_millis(300));
    let (phone_backend, recorded) = recorder();
    let phone = start(&phone_dir, "Phone", phone_backend);
    wait_for_within(
        &phone,
        "route restored after restart",
        Duration::from_secs(15),
        |s| {
            s.routes
                .iter()
                .any(|r| r.kind == RouteKind::ReceiveSystemAudio && r.status == RouteStatus::Active)
        },
    );
    wait_for_audio(&recorded, "audio after restart");

    // Turning the setting off forgets routes that were only saved because of it.
    let mut settings = phone.state().settings.clone();
    settings.resume_routes_on_start = false;
    let saved = rt.block_on(phone.update_settings(settings)).unwrap();
    assert!(saved.saved_routes.is_empty());
}

#[test]
fn flapping_connection_falls_back_to_stable() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let desk = start(&desk_dir, "Desk", sine());
    let phone = start(&phone_dir, "Phone", recorder().0);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (desk_id, phone_id) = pair(&rt, &desk, &phone);
    rt.block_on(phone.start_route(desk_id.clone(), RouteKind::ReceiveSystemAudio))
        .unwrap();

    // More than five drops within two minutes (plan §20).
    for i in 0..6 {
        desk.simulate_connection_loss(phone_id.clone()).unwrap();
        wait_for(&phone, &format!("drop {i} noticed"), |s| {
            !connected(s, &desk_id)
        });
        wait_for(&phone, &format!("reconnected after drop {i}"), |s| {
            connected(s, &desk_id)
        });
    }
    wait_for(&phone, "unstable connection notice", |s| {
        s.notices
            .iter()
            .any(|n| n.key == "notice.unstableConnection")
    });
    // The resumed route buffers like Stable (at least 60 ms) instead of Balanced (20 ms).
    wait_for_within(
        &phone,
        "route resumed with the Stable buffer",
        Duration::from_secs(15),
        |s| {
            s.routes.iter().any(|r| {
                r.kind == RouteKind::ReceiveSystemAudio
                    && r.status == RouteStatus::Active
                    && r.stats.buffer_ms >= 50.0
            })
        },
    );
}
