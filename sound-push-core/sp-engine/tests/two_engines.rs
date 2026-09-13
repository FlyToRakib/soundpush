//! Two engines in one process: QR pairing, route start, audio flowing, stop, reconnect.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sp_engine::sp_audio_io::AudioBackend;
use sp_engine::sp_audio_io::null::NullBackend;
use sp_engine::state::{ConnectionStatus, RouteStatus};
use sp_engine::{EngineConfig, EngineHandle, EngineState, PlatformHooks, RouteKind};

struct TestHooks {
    dir: PathBuf,
    name: &'static str,
    backend: Arc<NullBackend>,
}

impl PlatformHooks for TestHooks {
    fn data_dir(&self) -> PathBuf {
        self.dir.clone()
    }
    fn storage_key(&self) -> [u8; 32] {
        [42; 32]
    }
    fn platform(&self) -> &'static str {
        "linux"
    }
    fn default_device_name(&self) -> String {
        self.name.to_string()
    }
    fn audio_backend(&self) -> Arc<dyn AudioBackend> {
        self.backend.clone()
    }
}

fn config() -> EngineConfig {
    EngineConfig {
        app_version: "test".into(),
        port: 0,
        discovery: false,
        include_loopback: true,
    }
}

fn start(dir: &tempfile::TempDir, name: &'static str, backend: Arc<NullBackend>) -> EngineHandle {
    EngineHandle::start(
        Arc::new(TestHooks {
            dir: dir.path().to_path_buf(),
            name,
            backend,
        }),
        config(),
    )
    .expect("engine starts")
}

fn wait_for(engine: &EngineHandle, what: &str, pred: impl Fn(&EngineState) -> bool) -> Arc<EngineState> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = engine.state();
        if pred(&state) {
            return state;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}: {state:#?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn qr_pairing_route_and_audio() {
    let desk_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();

    let recorded = Arc::new(Mutex::new(Vec::new()));
    let desk = start(
        &desk_dir,
        "Desk",
        Arc::new(NullBackend {
            recorded: None,
            capture_frequency: 440.0,
        }),
    );
    let phone = start(
        &phone_dir,
        "Phone",
        Arc::new(NullBackend {
            recorded: Some(recorded.clone()),
            capture_frequency: 0.0,
        }),
    );
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Desk shows a QR code, phone scans it.
    let uri = rt.block_on(desk.start_pairing()).unwrap();
    rt.block_on(phone.pair_with_qr(uri)).unwrap();

    let desk_id = desk.state().local.device_id.clone();
    let phone_id = phone.state().local.device_id.clone();

    wait_for(&desk, "desk trusts phone", |s| {
        s.peers.iter().any(|p| p.device_id == phone_id && p.trusted && p.connection == ConnectionStatus::Connected)
    });
    wait_for(&phone, "phone trusts desk", |s| {
        s.peers.iter().any(|p| p.device_id == desk_id && p.trusted && p.connection == ConnectionStatus::Connected)
    });
    assert!(desk.state().pairing.qr_uri.is_none(), "QR secret is single-use");

    // Phone asks to hear the desk's system audio.
    let route_id = rt.block_on(phone.start_route(desk_id.clone(), RouteKind::ReceiveSystemAudio)).unwrap();
    wait_for(&desk, "desk route active", |s| {
        s.routes.iter().any(|r| r.kind == RouteKind::SendSystemAudio && r.status == RouteStatus::Active)
    });
    std::thread::sleep(Duration::from_millis(1500));

    let audio = recorded.lock().unwrap().clone();
    let tail = &audio[audio.len().saturating_sub(48_000)..];
    let peak = tail.iter().fold(0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.1, "phone should play the desk's sine, peak {peak}");

    let phone_state = phone.state();
    let route = phone_state.routes.iter().find(|r| r.route_id == route_id).unwrap();
    assert_eq!(route.stats.codec, "Opus");
    assert!(route.stats.latency_ms > 0.0);

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
    phone.respond_route_request(prompt.requests[0].request_id, true, false).unwrap();
    assert!(pending.join().unwrap().is_ok());

    // Stopping on one side stops on both.
    phone.stop_route(route_id).unwrap();
    wait_for(&desk, "desk system route stopped", |s| {
        !s.routes.iter().any(|r| r.kind == RouteKind::SendSystemAudio)
    });

    // Forgetting a device revokes access.
    desk.forget_device(phone_id.clone()).unwrap();
    wait_for(&desk, "phone forgotten", |s| !s.peers.iter().any(|p| p.device_id == phone_id && p.trusted));
}

#[test]
fn trusted_devices_reconnect_after_restart() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let backend = || Arc::new(NullBackend::default());

    let a = start(&a_dir, "A", backend());
    let b = start(&b_dir, "B", backend());
    let uri = rt.block_on(a.start_pairing()).unwrap();
    rt.block_on(b.pair_with_qr(uri)).unwrap();
    let a_id = a.state().local.device_id.clone();
    wait_for(&b, "paired", |s| s.peers.iter().any(|p| p.device_id == a_id && p.connection == ConnectionStatus::Connected));

    // Restart A on a new port; B only knows the old address, so A must reach B.
    drop(a);
    std::thread::sleep(Duration::from_millis(300));
    let a = start(&a_dir, "A", backend());
    assert_eq!(a.state().local.device_id, a_id, "identity persists across restarts");
    let b_id = b.state().local.device_id.clone();
    wait_for(&a, "A reconnects to B", |s| {
        s.peers.iter().any(|p| p.device_id == b_id && p.trusted && p.connection == ConnectionStatus::Connected)
    });
}
