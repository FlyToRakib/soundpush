//! The engine actor: owns all state and makes every decision.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use sp_audio_io::{AudioBackend, CaptureSource, DeviceKind, RenderTarget};
use sp_discovery::{Discovery, DiscoveryConfig, DiscoveryEvent, PeerAdvert};
use sp_media::codec::OpusApplication;
use sp_media::profile::build_profile;
use sp_protocol::control::{
    ControlMsg, ControlTarget, EndpointInfo, EndpointKind, Hello, MuteSet, PairRequest, PairResult, Platform,
    RouteAccept, RouteReject, RouteRequest, RouteStop, StatsReport, StopReason, StreamProfile, VolumeSet,
    control_msg::Body,
};
use sp_protocol::version::LOCAL_VERSIONS;
use sp_protocol::{Capabilities, Codec};
use sp_security::pairing::{QrPairingPayload, SAS_EXPORTER_LABEL, pairing_proof, sas_code, verify_pairing_proof};
use sp_security::trust::load_or_create_identity;
use sp_security::{DeviceId, DeviceIdentity, Fingerprint, PermissionKind, Permissions, Policy, TrustStore, TrustedDevice};
use sp_transport::{Endpoint, EndpointConfig, SecureConnection};
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, info};

use crate::error::{ErrorView, Severity};
use crate::net::{local_addresses, resolve, sort_candidates};
use crate::pipeline::monitor::MicMonitor;
use crate::pipeline::receiver::{Receiver, ReceiverConfig};
use crate::pipeline::sender::{DatagramSink, Sender, SenderConfig};
use crate::pipeline::{ReceiverControls, SenderControls};
use crate::platform::{KeepAlive, PlatformHooks};
use crate::reconnect::Backoff;
use crate::session::{self, Established, SessionCmd, SessionEvent};
use crate::settings::{MicMode, SavedRoute, Settings, SettingsStore, Visibility};
use crate::state::*;
use crate::{EngineConfig, EngineError};

mod local_audio;

type Reply<T> = oneshot::Sender<Result<T, EngineError>>;

pub(crate) enum Command {
    StartPairing(Reply<String>),
    StopPairing,
    PairWithQr { uri: String, reply: Reply<()> },
    PairWithDevice { device_id: String, reply: Reply<()> },
    PairWithAddress { address: String, reply: Reply<()> },
    ConfirmPairing { device_id: String, accept: bool },
    Connect { device_id: String },
    Disconnect { device_id: String },
    ForgetDevice { device_id: String },
    SetBlocked { device_id: String, blocked: bool },
    RenameDevice { device_id: String, alias: Option<String> },
    SetAutoConnect { device_id: String, enabled: bool },
    SetPermission { device_id: String, kind: PermissionKind, policy: Policy },
    StartRoute { device_id: String, kind: RouteKind, reply: Reply<String> },
    StopRoute { route_id: String },
    SetRouteVolume { route_id: String, volume: f32 },
    SetRouteMuted { route_id: String, muted: bool },
    SetRouteKeepRunning { route_id: String, keep: bool },
    SetPeerSpeakersMuted { device_id: String, muted: bool },
    RespondRouteRequest { request_id: u64, accept: bool, remember: bool },
    SetMicMuted { muted: bool },
    SetMicMonitor { enabled: bool },
    RefreshAudioDevices,
    AudioDevicesChanged { default_input: bool, default_output: bool },
    UpdateSettings { settings: Settings, reply: Reply<Settings> },
    DismissNotice { id: u64 },
    NetworkChanged,
    SetForeground { foreground: bool },
    /// `done` is signalled once sessions are closed and everything held has been released.
    Shutdown { done: Option<std::sync::mpsc::Sender<()>> },
}

enum Internal {
    Discovery(DiscoveryEvent),
    Incoming(SecureConnection),
    DialFailed { conn_id: u64, target: Option<DeviceId>, error: EngineError },
    Dialed { conn_id: u64, conn: SecureConnection, pair: Option<PairRequest> },
    AudioFailed { peer: DeviceId, route: u8, error: EngineError },
}

// ------------------------------------------------------------------ state types

struct Session {
    conn_id: u64,
    conn: SecureConnection,
    hello: Hello,
    dialed: bool,
    tx: mpsc::UnboundedSender<SessionCmd>,
    next_route: u8,
    connected_at: Instant,
}

impl Session {
    fn send(&self, body: Body) {
        let _ = self.tx.send(SessionCmd::Send(ControlMsg::new(0, body)));
    }

    fn alloc_route(&mut self) -> u8 {
        let id = self.next_route;
        self.next_route = self.next_route.wrapping_add(2);
        if self.next_route < 2 {
            self.next_route += 2;
        }
        id
    }
}

/// An authenticated connection whose peer is not trusted yet (pairing).
struct PendingPairing {
    conn_id: u64,
    conn: SecureConnection,
    hello: Hello,
    tx: mpsc::UnboundedSender<SessionCmd>,
    code: String,
    local_confirmed: bool,
    peer_confirmed: bool,
    /// Scanner side of QR pairing: the peer key was pinned from the QR code.
    qr_scanner: bool,
    created: Instant,
    dialed: bool,
}

struct Route {
    peer: DeviceId,
    id: u8,
    kind: RouteKind,
    status: RouteStatus,
    requested_locally: bool,
    profile: Option<StreamProfile>,
    started: Instant,
    started_unix: u64,
    sender: Option<Sender>,
    receiver: Option<Receiver>,
    sender_controls: Option<Arc<SenderControls>>,
    receiver_controls: Option<Arc<ReceiverControls>>,
    keep_running: bool,
    volume: f32,
    muted: bool,
    reply: Option<Reply<String>>,
    // adaptive bitrate & stats
    remote_stats: Option<StatsReport>,
    stable_secs: u32,
    /// Consecutive loss-free stats reports (turns automatic redundancy back off).
    clean_secs: u32,
    last_bytes: u64,
    bitrate_kbps: u32,
    last_received: u64,
    last_missing: u64,
    loss_pct: f64,
    paused_at: Option<Instant>,
    deadline: Option<Instant>,
}

impl Route {
    fn key(&self) -> String {
        route_key(&self.peer, self.id)
    }

    fn stop_pipelines(&mut self) {
        self.sender = None;
        self.receiver = None;
    }
}

struct PendingRequest {
    peer: DeviceId,
    request: RouteRequest,
    kind: RouteKind,
    expires: Instant,
    expires_unix: u64,
}

struct DialState {
    backoff: Backoff,
    next_attempt: Instant,
    in_flight: bool,
    /// The dial task in progress, cancelled once a session exists through the other direction.
    task: Option<tokio::task::AbortHandle>,
}

pub(crate) struct Actor {
    hooks: Arc<dyn PlatformHooks>,
    config: EngineConfig,
    backend: Arc<dyn AudioBackend>,
    identity: Arc<DeviceIdentity>,
    trust: TrustStore,
    settings: Settings,
    settings_store: SettingsStore,
    endpoint: Arc<Endpoint>,
    discovery: Option<Discovery>,
    internal_tx: mpsc::UnboundedSender<Internal>,
    session_tx: mpsc::UnboundedSender<SessionEvent>,
    state_tx: watch::Sender<Arc<EngineState>>,
    revision: u64,

    discovered: HashMap<DeviceId, PeerAdvert>,
    sessions: HashMap<DeviceId, Session>,
    /// conn_id → device for established sessions.
    conn_index: HashMap<u64, DeviceId>,
    pairings: HashMap<DeviceId, PendingPairing>,
    /// Untrusted listener-side connections waiting for a PairRequest.
    unpaired: HashMap<u64, (Established, Instant)>,
    dials: HashMap<DeviceId, DialState>,
    /// conn_id → trusted device a dial in progress is for; freed when the dial ends either way.
    dial_targets: HashMap<u64, DeviceId>,
    routes: Vec<Route>,
    requests: HashMap<u64, PendingRequest>,
    next_request_id: u64,
    next_conn_id: u64,
    qr: Option<QrPairingPayload>,
    /// Outgoing pairing dials by conn_id → true for QR pairing, false for code pairing.
    pair_dials: HashMap<u64, bool>,
    notices: Vec<NoticeView>,
    next_notice_id: u64,
    mic_muted: bool,
    monitor: Option<MicMonitor>,
    audio_devices: Vec<AudioDeviceView>,
    keep_alive: KeepAlive,
    foreground: bool,
    peer_speakers_muted: HashMap<DeviceId, bool>,
    /// Capability bits last announced to peers; a change is re-announced mid-session.
    announced_caps: u64,
    local_audio: local_audio::LocalAudio,
}

pub(crate) async fn spawn(
    hooks: Arc<dyn PlatformHooks>,
    config: EngineConfig,
) -> Result<(mpsc::UnboundedSender<Command>, watch::Receiver<Arc<EngineState>>), EngineError> {
    let data_dir = hooks.data_dir();
    std::fs::create_dir_all(&data_dir).map_err(|e| EngineError::Storage(e.to_string()))?;
    let key = hooks.storage_key();
    let (identity, identity_reset) = load_or_create_identity(&data_dir.join("identity.bin"), &key)?;
    let identity = Arc::new(identity);
    let (trust, trust_recovered) = TrustStore::load(data_dir.join("trust.bin"), key)?;
    let settings_store = SettingsStore::new(&data_dir);
    let (mut settings, settings_recovered) = settings_store.load();
    if settings.device_name.is_empty() {
        settings.device_name = hooks.default_device_name();
        let _ = settings_store.save(&settings);
    }

    let endpoint = Arc::new(Endpoint::bind(
        &identity,
        &EndpointConfig {
            preferred_port: config.port,
            ..EndpointConfig::default()
        },
    )?);

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let (internal_tx, mut internal_rx) = mpsc::unbounded_channel();
    let (session_tx, mut session_rx) = mpsc::unbounded_channel();
    let (state_tx, state_rx) = watch::channel(Arc::new(EngineState::default()));

    let backend = hooks.audio_backend();
    let mut actor = Actor {
        hooks,
        config,
        backend,
        identity,
        trust,
        settings,
        settings_store,
        endpoint: endpoint.clone(),
        discovery: None,
        internal_tx: internal_tx.clone(),
        session_tx,
        state_tx,
        revision: 0,
        discovered: HashMap::new(),
        sessions: HashMap::new(),
        conn_index: HashMap::new(),
        pairings: HashMap::new(),
        unpaired: HashMap::new(),
        dials: HashMap::new(),
        dial_targets: HashMap::new(),
        routes: Vec::new(),
        requests: HashMap::new(),
        next_request_id: 1,
        next_conn_id: 1,
        qr: None,
        pair_dials: HashMap::new(),
        notices: Vec::new(),
        next_notice_id: 1,
        mic_muted: false,
        monitor: None,
        audio_devices: Vec::new(),
        keep_alive: KeepAlive::default(),
        foreground: true,
        peer_speakers_muted: HashMap::new(),
        announced_caps: 0,
        local_audio: local_audio::LocalAudio::default(),
    };
    if identity_reset {
        actor.notice("notice.identityReset", vec![], Severity::Warning, None);
    }
    if trust_recovered {
        actor.notice("notice.trustStoreRecovered", vec![], Severity::Warning, None);
    }
    if settings_recovered {
        actor.notice("notice.settingsRecovered", vec![], Severity::Warning, None);
    }
    // Audio device listing is slow on some phones; it runs after start-up (first thing in the loop).

    // Accept loop.
    {
        let tx = internal_tx.clone();
        tokio::spawn(async move {
            while let Some(handshake) = endpoint.accept().await {
                // Each handshake on its own task: one stalled peer must not hold up the others.
                let tx = tx.clone();
                tokio::spawn(async move {
                    if let Ok(conn) = handshake.finish().await {
                        let _ = tx.send(Internal::Incoming(conn));
                    }
                });
            }
        });
    }

    // Discovery.
    if actor.config.discovery {
        let (discovery, mut events) = Discovery::start(actor.identity.clone(), actor.discovery_config());
        actor.discovery = Some(discovery);
        let tx = internal_tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = events.recv().await {
                if tx.send(Internal::Discovery(ev)).is_err() {
                    break;
                }
            }
        });
    }

    actor.announced_caps = actor.capability_bits().0;
    actor.publish();

    tokio::spawn(async move {
        actor.refresh_audio_devices();
        actor.publish();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                Some(cmd) = cmd_rx.recv() => {
                    if let Command::Shutdown { done } = cmd {
                        actor.shutdown();
                        if let Some(done) = done {
                            let _ = done.send(());
                        }
                        break;
                    }
                    actor.handle_command(cmd).await;
                }
                Some(ev) = internal_rx.recv() => actor.handle_internal(ev),
                Some(ev) = session_rx.recv() => actor.handle_session(ev),
                _ = tick.tick() => actor.tick(),
                else => break,
            }
            actor.publish();
        }
        info!("engine stopped");
    });

    Ok((cmd_tx, state_rx))
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn route_key(peer: &DeviceId, id: u8) -> String {
    format!("{}-{}", peer.to_hex(), id)
}

fn parse_device(id: &str) -> Result<DeviceId, EngineError> {
    DeviceId::from_hex(id).ok_or_else(|| EngineError::InvalidInput("device id".into()))
}

fn platform_enum(name: &str) -> Platform {
    match name {
        "windows" => Platform::Windows,
        "linux" => Platform::Linux,
        "macos" => Platform::MacOs,
        "android" => Platform::Android,
        "ios" => Platform::Ios,
        _ => Platform::Unknown,
    }
}

fn platform_name(p: i32) -> String {
    match Platform::try_from(p).unwrap_or(Platform::Unknown) {
        Platform::Windows => "windows",
        Platform::Linux => "linux",
        Platform::MacOs => "macos",
        Platform::Android => "android",
        Platform::Ios => "ios",
        Platform::Unknown => "unknown",
    }
    .to_string()
}

impl Actor {
    // ============================================================ helpers

    fn local_capabilities(&self) -> LocalCapabilities {
        let virtual_mic = self
            .hooks
            .virtual_mic_target(self.settings.desktop.virtual_mic_device.as_deref());
        let virtual_mic_device = match &virtual_mic {
            Some(sp_audio_io::RenderTarget::Output(name)) => Some(name.clone()),
            _ => None,
        };
        LocalCapabilities {
            system_audio: self.backend.supports_loopback(),
            app_audio: self.hooks.app_audio_source().is_some(),
            microphone: true,
            speaker: true,
            virtual_mic: virtual_mic.is_some(),
            virtual_mic_input: virtual_mic_device.as_deref().and_then(|d| self.hooks.virtual_cable_input(d)),
            virtual_mic_device,
        }
    }

    fn capability_bits(&self) -> Capabilities {
        let caps = self.local_capabilities();
        let mut bits = Capabilities::default()
            .with(Capabilities::CODEC_OPUS)
            .with(Capabilities::CODEC_PCM)
            .with(Capabilities::FEATURE_REMOTE_CONTROL)
            .with(Capabilities::FEATURE_MIC_MONITOR);
        if caps.system_audio {
            bits = bits.with(Capabilities::SOURCE_SYSTEM_AUDIO);
        }
        if caps.app_audio {
            bits = bits.with(Capabilities::SOURCE_APP_AUDIO);
        }
        if caps.microphone {
            bits = bits.with(Capabilities::SOURCE_MICROPHONE);
        }
        if caps.speaker {
            bits = bits.with(Capabilities::SINK_SPEAKER);
        }
        if caps.virtual_mic {
            bits = bits.with(Capabilities::SINK_VIRTUAL_MIC);
        }
        bits
    }

    fn local_hello(&self) -> Hello {
        let caps = self.local_capabilities();
        let mut endpoints = vec![
            endpoint_info("mic", "Microphone", EndpointKind::SourceMicrophone),
            endpoint_info("speaker", "Speakers", EndpointKind::SinkSpeaker),
        ];
        if caps.system_audio {
            endpoints.push(endpoint_info("system", "System audio", EndpointKind::SourceSystemAudio));
        }
        if caps.app_audio {
            endpoints.push(endpoint_info("apps", "App audio", EndpointKind::SourceAppAudio));
        }
        if caps.virtual_mic {
            endpoints.push(endpoint_info("virtual-mic", "SoundPush Microphone", EndpointKind::SinkVirtualMic));
        }
        Hello {
            protocol_min: LOCAL_VERSIONS.min.to_u32(),
            protocol_max: LOCAL_VERSIONS.max.to_u32(),
            app_version: self.config.app_version.clone(),
            device_id: Bytes::copy_from_slice(&self.identity.device_id().0),
            device_name: self.settings.device_name.clone(),
            platform: platform_enum(self.hooks.platform()) as i32,
            capabilities: self.capability_bits().0,
            resume_token: None,
            endpoints,
        }
    }

    fn discovery_config(&self) -> DiscoveryConfig {
        let visibility = if self.qr.is_some() {
            sp_discovery::Visibility::Everyone
        } else {
            match self.settings.visibility {
                Visibility::Everyone => sp_discovery::Visibility::Everyone,
                Visibility::TrustedOnly => sp_discovery::Visibility::TrustedOnly,
                Visibility::Hidden => sp_discovery::Visibility::Hidden,
            }
        };
        DiscoveryConfig {
            name: self.settings.device_name.clone(),
            platform: self.hooks.platform().to_string(),
            port: self.endpoint.local_port(),
            capabilities: self.capability_bits(),
            protocol_max: LOCAL_VERSIONS.max.to_u32(),
            visibility,
            beacon_interval: Duration::from_secs(3),
            browse: true,
        }
    }

    fn update_discovery(&self) {
        if let Some(d) = &self.discovery {
            d.update(self.discovery_config());
        }
    }

    fn notice(&mut self, key: &str, args: Vec<String>, severity: Severity, error: Option<&EngineError>) {
        let id = self.next_notice_id;
        self.next_notice_id += 1;
        self.notices.push(NoticeView {
            id,
            key: key.to_string(),
            args,
            severity,
            error: error.map(ErrorView::from),
            created_unix: now_unix(),
        });
        if self.notices.len() > 20 {
            self.notices.remove(0);
        }
    }

    fn error_notice(&mut self, error: &EngineError, args: Vec<String>) {
        // When the firewall blocks incoming connections, that is the fix to offer.
        if matches!(error, EngineError::Unreachable | EngineError::NetworkBlocked) && self.hooks.inbound_blocked() {
            let error = EngineError::FirewallBlocked;
            return self.notice(error.key(), args, error.severity(), Some(&error));
        }
        self.notice(error.key(), args, error.severity(), Some(error));
    }

    fn peer_name(&self, id: &DeviceId) -> String {
        if let Some(t) = self.trust.get(id) {
            return t.display_name().to_string();
        }
        if let Some(s) = self.sessions.get(id) {
            return s.hello.device_name.clone();
        }
        if let Some(p) = self.pairings.get(id) {
            return p.hello.device_name.clone();
        }
        self.discovered
            .get(id)
            .map(|a| a.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| id.display_code())
    }

    fn next_conn_id(&mut self) -> u64 {
        let id = self.next_conn_id;
        self.next_conn_id += 1;
        id
    }

    fn save_settings(&mut self) {
        if let Err(e) = self.settings_store.save(&self.settings) {
            self.error_notice(&e, vec![]);
        }
    }

    // ============================================================ commands

    async fn handle_command(&mut self, cmd: Command) {
        match cmd {
            Command::StartPairing(reply) => {
                let port = self.endpoint.local_port();
                let addrs = local_addresses(port, self.config.include_loopback);
                // The code carries only what the phone needs; the name arrives in the encrypted handshake.
                let payload = QrPairingPayload::new(self.identity.device_id(), addrs, now_unix());
                let uri = payload.to_uri();
                self.qr = Some(payload);
                self.update_discovery();
                let _ = reply.send(Ok(uri));
            }
            Command::StopPairing => {
                self.qr = None;
                self.update_discovery();
            }
            Command::PairWithQr { uri, reply } => {
                let result = self.pair_with_qr(&uri);
                let _ = reply.send(result);
            }
            Command::PairWithDevice { device_id, reply } => {
                let result = parse_device(&device_id).and_then(|id| {
                    let advert = self.discovered.get(&id).ok_or(EngineError::DeviceNotFound)?;
                    let addrs = advert.addresses.clone();
                    let conn_id = self.dial(addrs, None, Some(PairRequest { qr_proof: None }), None);
                    self.pair_dials.insert(conn_id, false);
                    Ok(())
                });
                let _ = reply.send(result);
            }
            Command::PairWithAddress { address, reply } => {
                let addrs = resolve(&address, sp_transport::DEFAULT_PORT).await;
                let result = if addrs.is_empty() {
                    Err(EngineError::InvalidInput("address".into()))
                } else {
                    let conn_id = self.dial(addrs, None, Some(PairRequest { qr_proof: None }), None);
                    self.pair_dials.insert(conn_id, false);
                    Ok(())
                };
                let _ = reply.send(result);
            }
            Command::ConfirmPairing { device_id, accept } => {
                if let Ok(id) = parse_device(&device_id) {
                    self.confirm_pairing(id, accept);
                }
            }
            Command::Connect { device_id } => {
                if let Ok(id) = parse_device(&device_id) {
                    self.dials.entry(id).or_insert_with(new_dial).next_attempt = Instant::now();
                    if let Some(d) = self.dials.get_mut(&id) {
                        d.backoff.reset();
                    }
                    self.try_dial_trusted(id);
                }
            }
            Command::Disconnect { device_id } => {
                if let Ok(id) = parse_device(&device_id) {
                    self.cancel_dial(&id);
                    self.dials.remove(&id);
                    if let Some(s) = self.sessions.remove(&id) {
                        self.conn_index.remove(&s.conn_id);
                        let _ = s.tx.send(SessionCmd::Close(StopReason::UserStopped));
                    }
                    self.remove_routes_for(id);
                    // Suppress auto reconnect for this device until the user connects again.
                    let _ = self.trust.update(&id, |d| d.auto_connect = false);
                }
            }
            Command::ForgetDevice { device_id } => {
                if let Ok(id) = parse_device(&device_id) {
                    self.revoke(id);
                    let _ = self.trust.remove(&id);
                }
            }
            Command::SetBlocked { device_id, blocked } => {
                if let Ok(id) = parse_device(&device_id) {
                    let _ = self.trust.update(&id, |d| d.blocked = blocked);
                    if blocked {
                        self.revoke(id);
                    }
                }
            }
            Command::RenameDevice { device_id, alias } => {
                if let Ok(id) = parse_device(&device_id) {
                    let alias = alias.map(|a| a.trim().chars().take(64).collect::<String>()).filter(|a| !a.is_empty());
                    let _ = self.trust.update(&id, |d| d.alias = alias);
                }
            }
            Command::SetAutoConnect { device_id, enabled } => {
                if let Ok(id) = parse_device(&device_id) {
                    let _ = self.trust.update(&id, |d| d.auto_connect = enabled);
                }
            }
            Command::SetPermission { device_id, kind, policy } => {
                if let Ok(id) = parse_device(&device_id) {
                    let _ = self.trust.update(&id, |d| d.permissions.set(kind, policy));
                    // Revocation takes effect immediately.
                    if policy == Policy::Deny {
                        self.enforce_permissions(id);
                    }
                }
            }
            Command::StartRoute { device_id, kind, reply } => match parse_device(&device_id) {
                Ok(id) => self.start_route(id, kind, reply),
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            },
            Command::StopRoute { route_id } => self.stop_route_by_key(&route_id, StopReason::UserStopped, true),
            Command::SetRouteVolume { route_id, volume } => {
                let volume = volume.clamp(0.0, 2.0);
                if let Some(r) = self.routes.iter_mut().find(|r| r.key() == route_id) {
                    r.volume = volume;
                    if let Some(c) = &r.receiver_controls {
                        c.volume.set(volume);
                    } else if let Some(s) = self.sessions.get(&r.peer) {
                        s.send(Body::VolumeSet(VolumeSet {
                            route: r.id as u32,
                            target: ControlTarget::RouteStream as i32,
                            gain: volume,
                        }));
                    }
                }
            }
            Command::SetRouteMuted { route_id, muted } => {
                let mic_muted = self.mic_muted;
                if let Some(r) = self.routes.iter_mut().find(|r| r.key() == route_id) {
                    r.muted = muted;
                    // The microphone mute keeps microphone routes silent whatever the route says.
                    let silent = muted || (r.kind.is_mic() && mic_muted);
                    if let Some(c) = &r.receiver_controls {
                        c.muted.store(silent, Ordering::Relaxed);
                    }
                    if let Some(c) = &r.sender_controls {
                        c.muted.store(silent, Ordering::Relaxed);
                    }
                    if r.receiver_controls.is_none() {
                        if let Some(s) = self.sessions.get(&r.peer) {
                            s.send(Body::MuteSet(MuteSet {
                                route: r.id as u32,
                                target: ControlTarget::RouteStream as i32,
                                muted,
                            }));
                        }
                    }
                }
            }
            Command::SetRouteKeepRunning { route_id, keep } => {
                if let Some(r) = self.routes.iter_mut().find(|r| r.key() == route_id) {
                    r.keep_running = keep;
                    let saved = SavedRoute {
                        peer_id: r.peer.to_hex(),
                        kind: r.kind,
                    };
                    self.settings.saved_routes.retain(|s| s != &saved);
                    if keep {
                        self.settings.saved_routes.push(saved);
                    }
                    self.save_settings();
                }
            }
            Command::SetPeerSpeakersMuted { device_id, muted } => {
                if let Ok(id) = parse_device(&device_id) {
                    if let Some(s) = self.sessions.get(&id) {
                        s.send(Body::MuteSet(MuteSet {
                            route: 0,
                            target: ControlTarget::DeviceSpeakers as i32,
                            muted,
                        }));
                        self.peer_speakers_muted.insert(id, muted);
                    }
                }
            }
            Command::RespondRouteRequest {
                request_id,
                accept,
                remember,
            } => self.respond_request(request_id, accept, remember),
            Command::SetMicMuted { muted } => self.set_mic_muted(muted),
            Command::SetMicMonitor { enabled } => self.set_monitor(enabled),
            Command::RefreshAudioDevices => self.refresh_audio_devices(),
            Command::AudioDevicesChanged {
                default_input,
                default_output,
            } => self.on_audio_devices_changed(default_input, default_output),
            Command::UpdateSettings { mut settings, reply } => {
                settings.sanitize();
                if settings.device_name.is_empty() {
                    settings.device_name = self.settings.device_name.clone();
                }
                let old = std::mem::replace(&mut self.settings, settings);
                self.save_settings();
                self.apply_settings(&old);
                let _ = reply.send(Ok(self.settings.clone()));
            }
            Command::DismissNotice { id } => self.notices.retain(|n| n.id != id),
            Command::NetworkChanged => {
                for d in self.dials.values_mut() {
                    d.backoff.reset();
                    d.next_attempt = Instant::now();
                }
                self.update_discovery();
            }
            Command::SetForeground { foreground } => self.foreground = foreground,
            Command::Shutdown { .. } => {}
        }
    }

    fn apply_settings(&mut self, old: &Settings) {
        if old.device_name != self.settings.device_name
            || old.visibility != self.settings.visibility
            || old.desktop.virtual_mic_device != self.settings.desktop.virtual_mic_device
        {
            self.update_discovery();
        }
        // Live updates to running routes.
        let (min, max, _) = self.settings.stream.latency_profile().params();
        for r in &self.routes {
            if let Some(c) = &r.receiver_controls {
                if !r.kind.is_mic() {
                    c.balance.set(self.settings.output.balance);
                    c.mono.store(self.settings.output.mono, Ordering::Relaxed);
                    c.av_offset_ms.store(self.settings.output.av_offset_ms, Ordering::Relaxed);
                }
                c.jitter_min_ms.store(min, Ordering::Relaxed);
                c.jitter_max_ms.store(max, Ordering::Relaxed);
            }
            if let Some(c) = &r.sender_controls {
                if r.kind.is_mic() {
                    c.gain_db.set(self.settings.mic.gain_db);
                    c.noise_suppression
                        .store(self.settings.mic.noise_suppression, Ordering::Relaxed);
                }
            }
        }
        if old.mic.monitor != self.settings.mic.monitor {
            self.set_monitor(self.settings.mic.monitor);
        } else if let Some(m) = &self.monitor {
            m.gain_db.set(self.settings.mic.gain_db);
        }
    }

    fn refresh_audio_devices(&mut self) {
        self.hooks.audio_devices_changed();
        // A broken audio stack must never stop the engine from starting.
        let backend = self.backend.clone();
        let listed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| backend.list_devices()));
        let devices = match listed {
            Ok(Ok(devices)) => devices,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "could not list audio devices");
                Vec::new()
            }
            Err(_) => {
                tracing::error!("audio backend panicked while listing devices");
                Vec::new()
            }
        };
        let hooks = self.hooks.clone();
        self.audio_devices = devices
            .into_iter()
            .map(|d| AudioDeviceView {
                virtual_cable: d.kind == DeviceKind::Output && hooks.virtual_cable_input(&d.name).is_some(),
                id: d.id,
                name: d.name,
                is_input: d.kind == DeviceKind::Input,
                is_default: d.is_default,
            })
            .collect();
    }

    fn set_monitor(&mut self, enabled: bool) {
        self.monitor = None;
        if enabled {
            match MicMonitor::start(
                self.backend.as_ref(),
                self.mic_source(),
                self.speaker_target(),
                self.settings.mic.gain_db,
            ) {
                Ok(m) => self.monitor = Some(m),
                Err(e) => self.error_notice(&e, vec![]),
            }
        }
        // Mobile shells start/stop the microphone recorder from the keep-alive signal.
        self.update_keep_alive();
    }

    fn mic_source(&self) -> CaptureSource {
        match &self.settings.mic.device {
            Some(d) => CaptureSource::Input(d.clone()),
            None => CaptureSource::DefaultInput,
        }
    }

    fn speaker_target(&self) -> RenderTarget {
        match &self.settings.output.device {
            Some(d) => RenderTarget::Output(d.clone()),
            None => RenderTarget::DefaultOutput,
        }
    }

    // ============================================================ dialing

    /// Dial candidates in order. Returns the conn_id the resulting session will use.
    fn dial(
        &mut self,
        mut addrs: Vec<SocketAddr>,
        pinned: Option<DeviceId>,
        pair: Option<PairRequest>,
        target: Option<DeviceId>,
    ) -> u64 {
        sort_candidates(&mut addrs, self.config.include_loopback);
        let conn_id = self.next_conn_id();
        let endpoint = self.endpoint.clone();
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            let mut last = EngineError::Unreachable;
            for addr in addrs.into_iter().take(8) {
                match tokio::time::timeout(Duration::from_secs(3), endpoint.connect(addr, pinned)).await {
                    Ok(Ok(conn)) => {
                        let _ = tx.send(Internal::Dialed { conn_id, conn, pair });
                        return;
                    }
                    Ok(Err(e)) => {
                        debug!(%addr, error = %e, "dial failed");
                        last = e.into();
                    }
                    Err(_) => last = EngineError::Unreachable,
                }
            }
            let _ = tx.send(Internal::DialFailed {
                conn_id,
                target,
                error: last,
            });
        });
        if let Some(id) = target {
            self.dial_targets.insert(conn_id, id);
            if let Some(d) = self.dials.get_mut(&id) {
                d.task = Some(task.abort_handle());
            }
        }
        conn_id
    }

    /// End a dial in progress for `id` (a session exists through the other direction, or the
    /// device is being disconnected) and free its dial state.
    fn cancel_dial(&mut self, id: &DeviceId) {
        if let Some(d) = self.dials.get_mut(id) {
            if let Some(task) = d.task.take() {
                task.abort();
            }
            d.in_flight = false;
        }
        self.dial_targets.retain(|_, target| target != id);
    }

    fn try_dial_trusted(&mut self, id: DeviceId) {
        if self.sessions.contains_key(&id) {
            return;
        }
        let Some(device) = self.trust.get(&id).cloned() else {
            return;
        };
        if device.blocked {
            return;
        }
        let state = self.dials.entry(id).or_insert_with(new_dial);
        if state.in_flight || Instant::now() < state.next_attempt {
            return;
        }
        state.in_flight = true;
        let mut addrs: Vec<SocketAddr> = self
            .discovered
            .get(&id)
            .map(|a| a.addresses.clone())
            .unwrap_or_default();
        addrs.extend(device.last_addresses.iter().filter_map(|a| a.parse::<SocketAddr>().ok()));
        if addrs.is_empty() {
            state.in_flight = false;
            let delay = state.backoff.next_delay();
            state.next_attempt = Instant::now() + delay;
            return;
        }
        // Pinned by device ID during the handshake; on_established then requires the exact stored key.
        self.dial(addrs, Some(id), None, Some(id));
    }

    fn pair_with_qr(&mut self, uri: &str) -> Result<(), EngineError> {
        let payload = QrPairingPayload::from_uri(uri).map_err(|_| EngineError::InvalidInput("pairing code".into()))?;
        // Expiry is enforced by the displaying device, which rejects stale secrets.
        if payload.device_id == self.identity.device_id() {
            return Err(EngineError::InvalidInput("this is your own pairing code".into()));
        }
        let proof = pairing_proof(&payload.secret, &self.identity.fingerprint(), &payload.device_id);
        let conn_id = self.dial(
            payload.addresses.clone(),
            Some(payload.device_id),
            Some(PairRequest {
                qr_proof: Some(Bytes::copy_from_slice(&proof)),
            }),
            Some(payload.device_id),
        );
        self.pair_dials.insert(conn_id, true);
        Ok(())
    }

    // ============================================================ internal events

    fn handle_internal(&mut self, ev: Internal) {
        match ev {
            Internal::Incoming(conn) => {
                let conn_id = self.next_conn_id();
                let hello = self.local_hello();
                tokio::spawn(session::run(conn, false, conn_id, hello, None, self.session_tx.clone()));
            }
            Internal::Dialed { conn_id, conn, pair } => {
                let hello = self.local_hello();
                tokio::spawn(session::run(conn, true, conn_id, hello, pair, self.session_tx.clone()));
            }
            Internal::DialFailed { conn_id, target, error } => {
                self.pair_dials.remove(&conn_id);
                self.dial_targets.remove(&conn_id);
                if let Some(id) = target {
                    if self.trust.get(&id).is_some() {
                        let state = self.dials.entry(id).or_insert_with(new_dial);
                        state.in_flight = false;
                        state.task = None;
                        let delay = state.backoff.next_delay();
                        state.next_attempt = Instant::now() + delay;
                        return;
                    }
                }
                self.error_notice(&error, vec![]);
            }
            Internal::Discovery(DiscoveryEvent::Updated(advert)) => {
                let id = advert.device_id;
                self.discovered.insert(id, advert);
                if let Some(d) = self.dials.get_mut(&id) {
                    if !d.in_flight {
                        d.backoff.reset();
                        d.next_attempt = Instant::now();
                    }
                }
            }
            Internal::Discovery(DiscoveryEvent::Lost(id)) => {
                self.discovered.remove(&id);
            }
            Internal::AudioFailed { peer, route, error } => {
                if self.reopen_on_default_device(peer, route) {
                    return;
                }
                let key = route_key(&peer, route);
                let name = self.peer_name(&peer);
                self.error_notice(&error, vec![name]);
                self.stop_route_by_key(&key, StopReason::AudioDeviceLost, true);
            }
        }
    }

    fn handle_session(&mut self, ev: SessionEvent) {
        match ev {
            SessionEvent::Established(est) => self.on_established(est),
            SessionEvent::Control { conn_id, msg } => self.on_control(conn_id, msg),
            SessionEvent::Closed { conn_id, reason } => self.on_closed(conn_id, reason),
            SessionEvent::HandshakeFailed { conn_id, incompatible } => {
                self.pair_dials.remove(&conn_id);
                // A dial whose QUIC handshake worked but whose Hello did not must free its dial
                // state, or the device is never dialed again.
                if let Some(id) = self.dial_targets.remove(&conn_id) {
                    if let Some(d) = self.dials.get_mut(&id) {
                        d.in_flight = false;
                        d.task = None;
                        d.next_attempt = Instant::now() + d.backoff.next_delay();
                    }
                }
                if incompatible {
                    self.error_notice(&EngineError::IncompatibleVersion, vec![]);
                }
            }
        }
    }

    fn on_established(&mut self, est: Established) {
        self.dial_targets.remove(&est.conn_id);
        let key = est.conn.peer_public_key();
        let id = Fingerprint::of_public_key(&key).device_id();

        if let Some(device) = self.trust.get(&id).cloned() {
            if device.blocked || device.public_key != key {
                let _ = est.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
                return;
            }
            self.promote_session(id, est);
            return;
        }

        // Untrusted peer.
        if est.dialed {
            match self.pair_dials.remove(&est.conn_id) {
                // We dialed to pair; the session already sent our PairRequest.
                Some(qr_scanner) => self.begin_pairing(id, est, qr_scanner),
                // A dial that was not for pairing reached an untrusted key (e.g. device was forgotten).
                None => {
                    let _ = est.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
                }
            }
        } else {
            // Wait for the peer's PairRequest (30 s).
            self.unpaired.insert(est.conn_id, (est, Instant::now()));
        }
    }

    fn begin_pairing(&mut self, id: DeviceId, est: Established, qr_scanner: bool) {
        // The code both users compare comes from the TLS session; without it there is nothing
        // to verify, so pairing stops rather than showing a predictable code.
        let Ok(exporter) = est.conn.export_keying_material(SAS_EXPORTER_LABEL, b"") else {
            let _ = est.tx.send(SessionCmd::Close(StopReason::Unspecified));
            self.error_notice(&EngineError::Internal("pairing code unavailable".into()), vec![]);
            return;
        };
        let code = sas_code(&exporter, &self.identity.fingerprint(), &est.conn.peer_fingerprint());
        if let Some(old) = self.pairings.remove(&id) {
            let _ = old.tx.send(SessionCmd::Close(StopReason::Superseded));
        }
        let name = est.hello.device_name.clone();
        self.pairings.insert(
            id,
            PendingPairing {
                conn_id: est.conn_id,
                conn: est.conn,
                hello: est.hello,
                tx: est.tx,
                code,
                local_confirmed: false,
                peer_confirmed: false,
                qr_scanner,
                created: Instant::now(),
                dialed: est.dialed,
            },
        );
        if !self.foreground {
            self.hooks.attention_needed("pairing.prompt", &name);
        }
    }

    fn confirm_pairing(&mut self, id: DeviceId, accept: bool) {
        let Some(p) = self.pairings.get_mut(&id) else {
            return;
        };
        p.local_confirmed = accept;
        let _ = p.tx.send(SessionCmd::Send(ControlMsg::new(0, Body::PairResult(PairResult { accepted: accept }))));
        if !accept {
            if let Some(p) = self.pairings.remove(&id) {
                let _ = p.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
            }
            return;
        }
        if p.peer_confirmed {
            self.complete_pairing(id);
        }
    }

    fn complete_pairing(&mut self, id: DeviceId) {
        let Some(p) = self.pairings.remove(&id) else {
            return;
        };
        let device = TrustedDevice {
            device_id: id,
            public_key: p.conn.peer_public_key(),
            name: p.hello.device_name.clone(),
            alias: None,
            platform: platform_name(p.hello.platform),
            permissions: Permissions::default(),
            auto_connect: true,
            blocked: false,
            paired_at_unix: now_unix(),
            last_seen_unix: now_unix(),
            last_addresses: vec![p.conn.remote_address().to_string()],
        };
        let name = device.name.clone();
        if let Err(e) = self.trust.upsert(device) {
            self.error_notice(&e.into(), vec![]);
            return;
        }
        self.notice("notice.pairingSuccess", vec![name], Severity::Info, None);
        let est = Established {
            conn_id: p.conn_id,
            conn: p.conn,
            hello: p.hello,
            dialed: p.dialed,
            tx: p.tx,
        };
        self.promote_session(id, est);
    }

    fn promote_session(&mut self, id: DeviceId, est: Established) {
        if let Some(old) = self.sessions.remove(&id) {
            // Deterministic tie-break for simultaneous dials: keep the connection
            // dialed by the device with the lower id; otherwise the newest wins.
            let local_lower = self.identity.device_id() < id;
            let old_preferred = old.dialed == local_lower;
            let new_preferred = est.dialed == local_lower;
            if old_preferred && !new_preferred && old.connected_at.elapsed() < Duration::from_secs(5) {
                let _ = est.tx.send(SessionCmd::Close(StopReason::Superseded));
                self.sessions.insert(id, old);
                // The rejected connection may be our own dial: free its dial state.
                self.cancel_dial(&id);
                return;
            }
            self.conn_index.remove(&old.conn_id);
            let _ = old.tx.send(SessionCmd::Close(StopReason::Superseded));
            self.pause_routes_for(id);
        }
        let _ = self.trust.update(&id, |d| {
            d.last_seen_unix = now_unix();
            d.name = est.hello.device_name.clone();
            let addr = est.conn.remote_address().to_string();
            d.last_addresses.retain(|a| a != &addr);
            d.last_addresses.insert(0, addr);
            d.last_addresses.truncate(4);
        });
        // A dial still walking candidates for this device is now pointless.
        self.cancel_dial(&id);
        if let Some(d) = self.dials.get_mut(&id) {
            d.backoff.reset();
        }
        self.conn_index.insert(est.conn_id, id);
        self.sessions.insert(
            id,
            Session {
                conn_id: est.conn_id,
                conn: est.conn,
                hello: est.hello,
                dialed: est.dialed,
                tx: est.tx,
                next_route: if est.dialed { 2 } else { 1 },
                connected_at: Instant::now(),
            },
        );
        self.resume_routes(id);
    }

    fn on_closed(&mut self, conn_id: u64, reason: StopReason) {
        self.unpaired.remove(&conn_id);
        if let Some((id, _)) = self.pairings.iter().find(|(_, p)| p.conn_id == conn_id).map(|(k, v)| (*k, v.conn_id)) {
            self.pairings.remove(&id);
            if reason == StopReason::PermissionDenied {
                self.error_notice(&EngineError::PairingRejected, vec![]);
            }
        }
        let Some(id) = self.conn_index.remove(&conn_id) else {
            return;
        };
        if self.sessions.get(&id).map(|s| s.conn_id) != Some(conn_id) {
            return;
        }
        self.sessions.remove(&id);
        info!(peer = %id.short(), ?reason, "session closed");

        match reason {
            StopReason::PermissionDenied | StopReason::PermissionRevoked => {
                self.remove_routes_for(id);
                self.cancel_dial(&id);
                self.dials.remove(&id);
                // The peer no longer trusts this device: redialing every second would only
                // repeat the refusal. Stop, and tell the user to pair again.
                let name = self.peer_name(&id);
                let _ = self.trust.update(&id, |d| d.auto_connect = false);
                self.notice("notice.peerForgotUs", vec![name], Severity::Warning, None);
            }
            StopReason::UserStopped | StopReason::Superseded => {
                self.remove_routes_for(id);
            }
            _ => {
                self.pause_routes_for(id);
                if self.trust.get(&id).is_some_and(|d| d.auto_connect) {
                    let state = self.dials.entry(id).or_insert_with(new_dial);
                    state.in_flight = false;
                    state.backoff.reset();
                    state.next_attempt = Instant::now() + Duration::from_millis(300);
                }
            }
        }
    }

    // ============================================================ control messages

    fn on_control(&mut self, conn_id: u64, msg: ControlMsg) {
        let Some(body) = msg.body else { return };

        // Messages from not-yet-trusted connections: only pairing is allowed.
        if let Some((est, _)) = self.unpaired.remove(&conn_id) {
            if let Body::PairRequest(req) = body {
                self.on_pair_request(est, req);
            } else {
                let _ = est.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
            }
            return;
        }
        if let Some(id) = self.pairings.iter().find(|(_, p)| p.conn_id == conn_id).map(|(k, _)| *k) {
            if let Body::PairResult(result) = body {
                let Some(p) = self.pairings.get_mut(&id) else { return };
                if !result.accepted {
                    if let Some(p) = self.pairings.remove(&id) {
                        let _ = p.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
                    }
                    self.error_notice(&EngineError::PairingRejected, vec![]);
                    return;
                }
                p.peer_confirmed = true;
                if p.qr_scanner || p.local_confirmed {
                    // QR scanner: the displayer verified our proof and we pinned its key.
                    if p.qr_scanner {
                        let _ = p.tx.send(SessionCmd::Send(ControlMsg::new(
                            0,
                            Body::PairResult(PairResult { accepted: true }),
                        )));
                    }
                    self.complete_pairing(id);
                }
            }
            return;
        }

        let Some(peer) = self.conn_index.get(&conn_id).copied() else {
            return;
        };
        match body {
            Body::RouteRequest(req) => self.on_route_request(peer, req),
            Body::RouteAccept(acc) => self.on_route_accept(peer, acc),
            Body::RouteReject(rej) => {
                let reason = StopReason::try_from(rej.reason).unwrap_or(StopReason::PeerStopped);
                let key = route_key(&peer, rej.route as u8);
                if let Some(pos) = self.routes.iter().position(|r| r.key() == key) {
                    let mut route = self.routes.remove(pos);
                    let err = match reason {
                        StopReason::PermissionDenied => EngineError::PeerDenied,
                        StopReason::UnsupportedEndpoint => EngineError::VirtualMicMissing,
                        StopReason::AudioDeviceLost | StopReason::DeviceBusy => {
                            EngineError::AudioDevice("remote".into())
                        }
                        _ => EngineError::PeerDenied,
                    };
                    if let Some(reply) = route.reply.take() {
                        let _ = reply.send(Err(err.clone()));
                    }
                    let name = self.peer_name(&peer);
                    self.error_notice(&err, vec![name]);
                }
            }
            Body::RouteStop(stop) => {
                let key = route_key(&peer, stop.route as u8);
                self.stop_route_by_key(&key, StopReason::PeerStopped, false);
            }
            Body::RouteUpdate(update) => {
                let key = route_key(&peer, update.route as u8);
                if let (Some(r), Some(p)) = (self.routes.iter().find(|r| r.key() == key), update.profile) {
                    if let Some(c) = &r.receiver_controls {
                        c.jitter_min_ms.store(p.jitter_min_ms, Ordering::Relaxed);
                        c.jitter_max_ms.store(p.jitter_max_ms, Ordering::Relaxed);
                    }
                    if let Some(c) = &r.sender_controls {
                        c.bitrate.store(p.bitrate, Ordering::Relaxed);
                    }
                }
            }
            Body::VolumeSet(v) => self.on_remote_volume(peer, v),
            Body::MuteSet(m) => self.on_remote_mute(peer, m),
            Body::StatsReport(stats) => self.on_stats(peer, stats),
            Body::Ping(_) | Body::Pong(_) | Body::Notice(_) | Body::PermissionChanged(_) => {}
            Body::Hello(hello) => {
                // Mid-session Hello only updates what the peer can do; identity and
                // protocol versions were fixed by the authenticated handshake.
                if let Some(s) = self.sessions.get_mut(&peer) {
                    if s.conn_id == conn_id && hello.device_id == s.hello.device_id {
                        s.hello.capabilities = hello.capabilities;
                        s.hello.endpoints = hello.endpoints;
                    }
                }
            }
            Body::Goodbye(_) | Body::PairRequest(_) | Body::PairResult(_) => {}
        }
    }

    fn on_pair_request(&mut self, est: Established, req: PairRequest) {
        let id = est.conn.peer_fingerprint().device_id();
        match (req.qr_proof, self.qr.as_ref()) {
            (Some(proof), Some(qr)) if !qr.is_expired(now_unix()) => {
                let ok = verify_pairing_proof(&qr.secret, &est.conn.peer_fingerprint(), &self.identity.device_id(), &proof)
                    .is_ok();
                if !ok {
                    let _ = est.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
                    return;
                }
                // Single-use QR secret.
                self.qr = None;
                self.update_discovery();
                let tx = est.tx.clone();
                self.begin_pairing(id, est, false);
                if let Some(p) = self.pairings.get_mut(&id) {
                    p.local_confirmed = true;
                    p.peer_confirmed = true;
                }
                let _ = tx.send(SessionCmd::Send(ControlMsg::new(0, Body::PairResult(PairResult { accepted: true }))));
                self.complete_pairing(id);
            }
            (None, Some(_)) => {
                // Code pairing is only accepted while pairing mode is open on this device.
                self.begin_pairing(id, est, false);
            }
            _ => {
                let _ = est.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
            }
        }
    }

    // ============================================================ routes

    fn start_route(&mut self, peer: DeviceId, kind: RouteKind, reply: Reply<String>) {
        if let Err(e) = self.check_local_capability(kind) {
            let _ = reply.send(Err(e));
            return;
        }
        if let Some(existing) = self.routes.iter().find(|r| r.peer == peer && r.kind == kind && r.status != RouteStatus::Stopped) {
            let _ = reply.send(Ok(existing.key()));
            return;
        }
        let profile = self.profile_for(kind);
        let Some(session) = self.sessions.get_mut(&peer) else {
            let _ = reply.send(Err(if self.trust.get(&peer).is_some() {
                EngineError::Unreachable
            } else {
                EngineError::NotPaired
            }));
            return;
        };
        let id = session.alloc_route();
        let (source, sink) = kind.endpoints();
        session.send(Body::RouteRequest(RouteRequest {
            route: id as u32,
            source_endpoint: source.into(),
            sink_endpoint: sink.into(),
            requester_is_source: kind.local_is_source(),
            profile: Some(profile.clone()),
        }));
        let mut route = new_route(peer, id, kind, true, self.settings.output.volume);
        route.status = RouteStatus::Requesting;
        route.profile = Some(profile);
        route.reply = Some(reply);
        route.deadline = Some(Instant::now() + Duration::from_secs(35));
        self.routes.push(route);
    }

    fn check_local_capability(&self, kind: RouteKind) -> Result<(), EngineError> {
        let caps = self.local_capabilities();
        match kind {
            RouteKind::SendSystemAudio if !caps.system_audio => Err(EngineError::LoopbackUnsupported),
            RouteKind::SendAppAudio if !caps.app_audio => Err(EngineError::LoopbackUnsupported),
            RouteKind::ReceiveMicToVirtualMic if !caps.virtual_mic => Err(EngineError::VirtualMicMissing),
            RouteKind::SendMicToVirtualMic | RouteKind::SendMicToSpeaker if !self.hooks.microphone_permitted() => {
                Err(EngineError::MicPermissionDenied)
            }
            _ => Ok(()),
        }
    }

    fn profile_for(&self, kind: RouteKind) -> StreamProfile {
        let s = &self.settings.stream;
        let channels = if kind.is_mic() { 1 } else { 2 };
        let mut p = build_profile(s.latency_profile(), s.quality(), channels, s.redundancy);
        if kind.is_mic() && p.frame_us < 10_000 {
            // Noise suppression works on 10 ms frames.
            p.frame_us = 10_000;
        }
        p
    }

    fn on_route_request(&mut self, peer: DeviceId, req: RouteRequest) {
        let local_is_source = !req.requester_is_source;
        let Some(kind) = RouteKind::from_endpoints(&req.source_endpoint, &req.sink_endpoint, local_is_source) else {
            self.reject(peer, req.route, StopReason::UnsupportedEndpoint);
            return;
        };
        if let Err(e) = self.check_local_capability(kind) {
            let reason = match e {
                EngineError::MicPermissionDenied => StopReason::PermissionDenied,
                _ => StopReason::UnsupportedEndpoint,
            };
            self.reject(peer, req.route, reason);
            return;
        }
        let permission = if local_is_source {
            if kind.is_mic() { PermissionKind::UseMyMicrophone } else { PermissionKind::ReceiveMyAudio }
        } else {
            PermissionKind::SendAudioToMe
        };
        let policy = self
            .trust
            .get(&peer)
            .map(|d| d.permissions.policy(permission))
            .unwrap_or(Policy::Deny);
        match policy {
            Policy::Allow => self.accept_route(peer, req, kind),
            Policy::Deny => self.reject(peer, req.route, StopReason::PermissionDenied),
            Policy::Ask => {
                let request_id = self.next_request_id;
                self.next_request_id += 1;
                self.requests.insert(
                    request_id,
                    PendingRequest {
                        peer,
                        request: req,
                        kind,
                        expires: Instant::now() + Duration::from_secs(30),
                        expires_unix: now_unix() + 30,
                    },
                );
                let name = self.peer_name(&peer);
                self.hooks.attention_needed("route.request", &name);
            }
        }
    }

    fn respond_request(&mut self, request_id: u64, accept: bool, remember: bool) {
        let Some(pending) = self.requests.remove(&request_id) else {
            return;
        };
        if remember {
            let kind = if pending.kind.local_is_source() {
                if pending.kind.is_mic() { PermissionKind::UseMyMicrophone } else { PermissionKind::ReceiveMyAudio }
            } else {
                PermissionKind::SendAudioToMe
            };
            let policy = if accept { Policy::Allow } else { Policy::Deny };
            let _ = self.trust.update(&pending.peer, |d| d.permissions.set(kind, policy));
        }
        if accept {
            self.accept_route(pending.peer, pending.request, pending.kind);
        } else {
            self.reject(pending.peer, pending.request.route, StopReason::PermissionDenied);
        }
    }

    fn reject(&self, peer: DeviceId, route: u32, reason: StopReason) {
        if let Some(s) = self.sessions.get(&peer) {
            s.send(Body::RouteReject(RouteReject {
                route,
                reason: reason as i32,
            }));
        }
    }

    fn accept_route(&mut self, peer: DeviceId, req: RouteRequest, kind: RouteKind) {
        let mut profile = req.profile.clone().unwrap_or_else(|| self.profile_for(kind));
        sanitize_profile(&mut profile);
        if !kind.local_is_source() {
            // The receiving side's latency preference wins.
            let local = self.profile_for(kind);
            profile.jitter_min_ms = local.jitter_min_ms;
            profile.jitter_max_ms = local.jitter_max_ms;
        }
        let id = req.route as u8;
        let mut route = new_route(peer, id, kind, false, self.settings.output.volume);
        route.profile = Some(profile.clone());
        if let Err(e) = self.start_pipelines(&mut route) {
            let reason = match e {
                EngineError::MicPermissionDenied => StopReason::PermissionDenied,
                EngineError::VirtualMicMissing | EngineError::LoopbackUnsupported => StopReason::UnsupportedEndpoint,
                _ => StopReason::AudioDeviceLost,
            };
            self.reject(peer, req.route, reason);
            let name = self.peer_name(&peer);
            self.error_notice(&e, vec![name]);
            return;
        }
        route.status = RouteStatus::Active;
        if let Some(s) = self.sessions.get(&peer) {
            s.send(Body::RouteAccept(RouteAccept {
                route: req.route,
                profile: Some(profile),
            }));
        }
        self.routes.retain(|r| !(r.peer == peer && r.kind == kind && r.status == RouteStatus::Paused));
        self.routes.push(route);
        self.update_keep_alive();
    }

    fn on_route_accept(&mut self, peer: DeviceId, acc: RouteAccept) {
        let key = route_key(&peer, acc.route as u8);
        let Some(pos) = self.routes.iter().position(|r| r.key() == key) else {
            return;
        };
        let mut route = self.routes.remove(pos);
        if let Some(mut p) = acc.profile {
            sanitize_profile(&mut p);
            route.profile = Some(p);
        }
        match self.start_pipelines(&mut route) {
            Ok(()) => {
                route.status = RouteStatus::Active;
                route.started = Instant::now();
                route.started_unix = now_unix();
                route.deadline = None;
                if let Some(reply) = route.reply.take() {
                    let _ = reply.send(Ok(route.key()));
                }
                self.routes.push(route);
                self.update_keep_alive();
            }
            Err(e) => {
                if let Some(s) = self.sessions.get(&peer) {
                    s.send(Body::RouteStop(RouteStop {
                        route: acc.route,
                        reason: StopReason::AudioDeviceLost as i32,
                    }));
                }
                if let Some(reply) = route.reply.take() {
                    let _ = reply.send(Err(e.clone()));
                }
                let name = self.peer_name(&peer);
                self.error_notice(&e, vec![name]);
            }
        }
    }

    fn start_pipelines(&mut self, route: &mut Route) -> Result<(), EngineError> {
        let session = self.sessions.get(&route.peer).ok_or(EngineError::Unreachable)?;
        let profile = route.profile.clone().ok_or_else(|| EngineError::Internal("missing profile".into()))?;
        let peer = route.peer;
        let id = route.id;
        let failed_tx = self.internal_tx.clone();
        let on_error = Box::new(move |e: sp_audio_io::AudioError| {
            let _ = failed_tx.send(Internal::AudioFailed {
                peer,
                route: id,
                error: e.into(),
            });
        });

        if route.kind.local_is_source() {
            let source = match route.kind.endpoints().0 {
                "system" => self.system_audio_source(),
                "apps" => self.hooks.app_audio_source().ok_or(EngineError::LoopbackUnsupported)?,
                _ => self.mic_source(),
            };
            let is_mic = route.kind.is_mic();
            let controls = Arc::new(SenderControls::new(
                if is_mic { self.settings.mic.gain_db } else { 0.0 },
                is_mic && self.settings.mic.noise_suppression,
                profile.bitrate,
            ));
            controls.muted.store(route.muted || (is_mic && self.mic_muted), Ordering::Relaxed);
            controls.redundancy.store(profile.redundancy, Ordering::Relaxed);
            let sink: Arc<dyn DatagramSink> = Arc::new(session.conn.clone());
            let sender = Sender::start(
                self.backend.as_ref(),
                SenderConfig {
                    route: id,
                    profile,
                    application: if is_mic { OpusApplication::Voip } else { OpusApplication::LowDelay },
                    source,
                },
                controls.clone(),
                sink,
                on_error,
            )?;
            if route.kind == RouteKind::SendSystemAudio && self.settings.capture.mute_local_speakers {
                self.hooks.set_speakers_muted(true);
            }
            route.sender_controls = Some(controls);
            route.sender = Some(sender);
        } else {
            let target = match route.kind.endpoints().1 {
                "virtual-mic" => self
                    .hooks
                    .virtual_mic_target(self.settings.desktop.virtual_mic_device.as_deref())
                    .ok_or(EngineError::VirtualMicMissing)?,
                _ => self.speaker_target(),
            };
            let controls = Arc::new(ReceiverControls::new(
                if route.kind.is_mic() { 1.0 } else { route.volume },
                profile.jitter_min_ms,
                profile.jitter_max_ms,
            ));
            controls
                .muted
                .store(route.muted || (route.kind.is_mic() && self.mic_muted), Ordering::Relaxed);
            if !route.kind.is_mic() {
                controls.balance.set(self.settings.output.balance);
                controls.mono.store(self.settings.output.mono, Ordering::Relaxed);
                controls.av_offset_ms.store(self.settings.output.av_offset_ms, Ordering::Relaxed);
            }
            let (receiver, sink) = Receiver::start(
                self.backend.as_ref(),
                ReceiverConfig { profile, target },
                controls.clone(),
                on_error,
            )?;
            let _ = session.tx.send(SessionCmd::AddSink(id, sink));
            route.receiver_controls = Some(controls);
            route.receiver = Some(receiver);
        }
        Ok(())
    }

    fn stop_route_by_key(&mut self, key: &str, reason: StopReason, notify_peer: bool) {
        let Some(pos) = self.routes.iter().position(|r| r.key() == key) else {
            return;
        };
        let mut route = self.routes.remove(pos);
        route.stop_pipelines();
        if let Some(s) = self.sessions.get(&route.peer) {
            let _ = s.tx.send(SessionCmd::RemoveSink(route.id));
            if notify_peer {
                s.send(Body::RouteStop(RouteStop {
                    route: route.id as u32,
                    reason: reason as i32,
                }));
            }
        }
        if let Some(reply) = route.reply.take() {
            let _ = reply.send(Err(EngineError::RouteNotFound));
        }
        if route.kind == RouteKind::SendSystemAudio && self.settings.capture.mute_local_speakers {
            self.hooks.set_speakers_muted(false);
        }
        if reason == StopReason::UserStopped {
            let saved = SavedRoute {
                peer_id: route.peer.to_hex(),
                kind: route.kind,
            };
            if self.settings.saved_routes.contains(&saved) {
                self.settings.saved_routes.retain(|s| s != &saved);
                self.save_settings();
            }
        }
        self.update_keep_alive();
    }

    fn pause_routes_for(&mut self, peer: DeviceId) {
        for r in self.routes.iter_mut().filter(|r| r.peer == peer) {
            r.stop_pipelines();
            if r.status == RouteStatus::Active {
                r.status = RouteStatus::Paused;
                r.paused_at = Some(Instant::now());
            } else if let Some(reply) = r.reply.take() {
                let _ = reply.send(Err(EngineError::Unreachable));
                r.status = RouteStatus::Stopped;
            }
        }
        self.routes.retain(|r| r.status != RouteStatus::Stopped);
        self.update_keep_alive();
    }

    fn remove_routes_for(&mut self, peer: DeviceId) {
        let keys: Vec<String> = self.routes.iter().filter(|r| r.peer == peer).map(Route::key).collect();
        for k in keys {
            self.stop_route_by_key(&k, StopReason::PeerStopped, false);
        }
    }

    /// Re-request routes after a reconnect (only the side that originally requested does this).
    fn resume_routes(&mut self, peer: DeviceId) {
        let mut kinds: Vec<RouteKind> = self
            .routes
            .iter()
            .filter(|r| r.peer == peer && r.status == RouteStatus::Paused && r.requested_locally)
            .map(|r| r.kind)
            .collect();
        self.routes.retain(|r| !(r.peer == peer && r.status == RouteStatus::Paused && r.requested_locally));
        let peer_hex = peer.to_hex();
        for saved in &self.settings.saved_routes {
            if saved.peer_id == peer_hex && !kinds.contains(&saved.kind) {
                kinds.push(saved.kind);
            }
        }
        for kind in kinds {
            let (tx, _rx) = oneshot::channel();
            self.start_route(peer, kind, tx);
            if let Some(r) = self.routes.iter_mut().rev().find(|r| r.peer == peer && r.kind == kind) {
                r.keep_running = self
                    .settings
                    .saved_routes
                    .iter()
                    .any(|s| s.peer_id == peer_hex && s.kind == kind);
            }
        }
    }

    fn enforce_permissions(&mut self, peer: DeviceId) {
        let Some(perms) = self.trust.get(&peer).map(|d| d.permissions) else {
            return;
        };
        let keys: Vec<String> = self
            .routes
            .iter()
            .filter(|r| r.peer == peer)
            .filter(|r| {
                let kind = if r.kind.local_is_source() {
                    if r.kind.is_mic() { PermissionKind::UseMyMicrophone } else { PermissionKind::ReceiveMyAudio }
                } else {
                    PermissionKind::SendAudioToMe
                };
                !r.requested_locally && perms.policy(kind) == Policy::Deny
            })
            .map(Route::key)
            .collect();
        for k in keys {
            self.stop_route_by_key(&k, StopReason::PermissionRevoked, true);
        }
    }

    fn revoke(&mut self, id: DeviceId) {
        self.remove_routes_for(id);
        self.cancel_dial(&id);
        self.dials.remove(&id);
        if let Some(s) = self.sessions.remove(&id) {
            self.conn_index.remove(&s.conn_id);
            let _ = s.tx.send(SessionCmd::Close(StopReason::PermissionRevoked));
        }
    }

    fn on_remote_volume(&mut self, peer: DeviceId, v: VolumeSet) {
        if !self.peer_may_control(peer) {
            return;
        }
        let key = route_key(&peer, v.route as u8);
        if let Some(r) = self.routes.iter_mut().find(|r| r.key() == key) {
            r.volume = v.gain.clamp(0.0, 2.0);
            if let Some(c) = &r.receiver_controls {
                c.volume.set(r.volume);
            }
        }
    }

    fn on_remote_mute(&mut self, peer: DeviceId, m: MuteSet) {
        if !self.peer_may_control(peer) {
            return;
        }
        if m.target == ControlTarget::DeviceSpeakers as i32 {
            if !self.hooks.set_speakers_muted(m.muted) {
                let name = self.peer_name(&peer);
                self.notice("notice.muteSpeakersUnsupported", vec![name], Severity::Info, None);
            }
            return;
        }
        let key = route_key(&peer, m.route as u8);
        if let Some(r) = self.routes.iter_mut().find(|r| r.key() == key) {
            r.muted = m.muted;
            if let Some(c) = &r.receiver_controls {
                c.muted.store(m.muted, Ordering::Relaxed);
            }
        }
    }

    fn peer_may_control(&self, peer: DeviceId) -> bool {
        self.trust
            .get(&peer)
            .is_some_and(|d| d.permissions.policy(PermissionKind::ControlMe) == Policy::Allow)
    }

    fn on_stats(&mut self, peer: DeviceId, stats: StatsReport) {
        let key = route_key(&peer, stats.route as u8);
        let Some(r) = self.routes.iter_mut().find(|r| r.key() == key) else {
            return;
        };
        let total = stats.packets_received.saturating_add(stats.packets_lost);
        r.loss_pct = if total > 0 { stats.packets_lost as f64 * 100.0 / total as f64 } else { 0.0 };
        if let (Some(c), Some(p)) = (&r.sender_controls, &r.profile) {
            if p.adaptive_bitrate && p.codec == Codec::Opus as u32 {
                let current = c.bitrate.load(Ordering::Relaxed);
                if r.loss_pct > 2.0 {
                    c.bitrate.store((current * 3 / 4).max(32_000), Ordering::Relaxed);
                    c.expected_loss_pct.store(r.loss_pct.round() as u32, Ordering::Relaxed);
                    r.stable_secs = 0;
                } else if r.loss_pct < 0.5 {
                    r.stable_secs += 1;
                    if r.stable_secs >= 10 {
                        c.bitrate.store((current * 5 / 4).min(p.bitrate.max(128_000)), Ordering::Relaxed);
                        c.expected_loss_pct.store(0, Ordering::Relaxed);
                        r.stable_secs = 0;
                    }
                }
            }
        }
        // Automatic redundancy: on above 1 % loss, off after 10 clean seconds (unless forced by the profile).
        if let (Some(c), Some(p)) = (&r.sender_controls, &r.profile) {
            if r.loss_pct > 1.0 {
                c.redundancy.store(true, Ordering::Relaxed);
                r.clean_secs = 0;
            } else if r.loss_pct == 0.0 {
                r.clean_secs += 1;
                if r.clean_secs >= 10 && !p.redundancy {
                    c.redundancy.store(false, Ordering::Relaxed);
                }
            }
        }
        r.remote_stats = Some(stats);
    }

    // ============================================================ periodic work

    fn tick(&mut self) {
        let now = Instant::now();

        // Route deadlines, paused route expiry, stats reports.
        let expired: Vec<String> = self
            .routes
            .iter()
            .filter(|r| {
                r.deadline.is_some_and(|d| now > d)
                    || (r.status == RouteStatus::Paused
                        && !r.keep_running
                        && r.paused_at.is_some_and(|p| now.duration_since(p) > Duration::from_secs(120)))
            })
            .map(Route::key)
            .collect();
        for k in expired {
            if let Some(r) = self.routes.iter_mut().find(|r| r.key() == k) {
                if let Some(reply) = r.reply.take() {
                    let _ = reply.send(Err(EngineError::Unreachable));
                }
            }
            self.stop_route_by_key(&k, StopReason::Timeout, true);
        }

        for r in &mut self.routes {
            if let Some(c) = &r.receiver_controls {
                let received = c.packets_received.load(Ordering::Relaxed);
                let missing = c.packets_missing.load(Ordering::Relaxed);
                let (dr, dm) = (received.saturating_sub(r.last_received), missing.saturating_sub(r.last_missing));
                r.last_received = received;
                r.last_missing = missing;
                r.loss_pct = if dr + dm > 0 { dm as f64 * 100.0 / (dr + dm) as f64 } else { 0.0 };
                let bytes = c.bytes_received.load(Ordering::Relaxed);
                r.bitrate_kbps = (bytes.saturating_sub(r.last_bytes) * 8 / 1000) as u32;
                r.last_bytes = bytes;
                if let Some(s) = self.sessions.get(&r.peer) {
                    s.send(Body::StatsReport(StatsReport {
                        route: r.id as u32,
                        packets_received: dr as u32,
                        packets_lost: dm as u32,
                        jitter_us: (c.jitter_ms.get() * 1000.0) as u32,
                        buffer_ms: c.buffer_ms.get() as u32,
                        underruns: c.underruns.load(Ordering::Relaxed) as u32,
                        drift_ppm: c.drift_ppm.load(Ordering::Relaxed),
                        output_latency_ms: c.device_latency_ms.load(Ordering::Relaxed),
                    }));
                }
            }
            if let Some(c) = &r.sender_controls {
                let bytes = c.bytes_sent.load(Ordering::Relaxed);
                r.bitrate_kbps = (bytes.saturating_sub(r.last_bytes) * 8 / 1000) as u32;
                r.last_bytes = bytes;
            }
        }

        // Capabilities can change while connected (e.g. a virtual microphone was installed).
        // Re-send Hello so peers enable or disable the matching tasks without reconnecting.
        let caps = self.capability_bits().0;
        if caps != self.announced_caps {
            self.announced_caps = caps;
            let hello = self.local_hello();
            for s in self.sessions.values() {
                s.send(Body::Hello(hello.clone()));
            }
            self.update_discovery();
        }

        self.check_virtual_mic_use();

        // Pending prompts expire.
        let expired_requests: Vec<u64> = self.requests.iter().filter(|(_, r)| now > r.expires).map(|(k, _)| *k).collect();
        for id in expired_requests {
            if let Some(p) = self.requests.remove(&id) {
                self.reject(p.peer, p.request.route, StopReason::Timeout);
            }
        }
        let stale_unpaired: Vec<u64> = self
            .unpaired
            .iter()
            .filter(|(_, (_, t))| now.duration_since(*t) > Duration::from_secs(30))
            .map(|(k, _)| *k)
            .collect();
        for id in stale_unpaired {
            if let Some((est, _)) = self.unpaired.remove(&id) {
                let _ = est.tx.send(SessionCmd::Close(StopReason::Timeout));
            }
        }
        let stale_pairings: Vec<DeviceId> = self
            .pairings
            .iter()
            .filter(|(_, p)| now.duration_since(p.created) > Duration::from_secs(120))
            .map(|(k, _)| *k)
            .collect();
        for id in stale_pairings {
            if let Some(p) = self.pairings.remove(&id) {
                let _ = p.tx.send(SessionCmd::Close(StopReason::Timeout));
            }
        }
        if self.qr.as_ref().is_some_and(|q| q.is_expired(now_unix())) {
            self.qr = None;
            self.update_discovery();
        }

        // Auto-connect trusted devices.
        if self.settings.auto_connect_trusted {
            let candidates: Vec<DeviceId> = self
                .trust
                .list()
                .filter(|d| d.auto_connect && !d.blocked && !self.sessions.contains_key(&d.device_id))
                .map(|d| d.device_id)
                .collect();
            for id in candidates {
                self.try_dial_trusted(id);
            }
        }
    }

    fn update_keep_alive(&mut self) {
        let mut ka = KeepAlive::default();
        for r in &self.routes {
            if r.status != RouteStatus::Active {
                continue;
            }
            match r.kind {
                RouteKind::SendMicToSpeaker | RouteKind::SendMicToVirtualMic => ka.microphone = true,
                RouteKind::SendAppAudio => ka.app_audio_capture = true,
                RouteKind::SendSystemAudio => {}
                _ => ka.playback = true,
            }
        }
        if self.monitor.is_some() {
            ka.microphone = true;
        }
        if ka != self.keep_alive {
            self.keep_alive = ka;
            self.hooks.keep_alive(ka);
        }
    }

    fn shutdown(&mut self) {
        // Sending this computer's audio may have muted its speakers: never leave them that way.
        if self.settings.capture.mute_local_speakers && self.routes.iter().any(|r| r.kind == RouteKind::SendSystemAudio) {
            self.hooks.set_speakers_muted(false);
        }
        for (_, s) in self.sessions.drain() {
            let _ = s.tx.send(SessionCmd::Close(StopReason::UserStopped));
        }
        for r in &mut self.routes {
            r.stop_pipelines();
        }
        self.routes.clear();
        self.monitor = None;
        self.endpoint.close();
    }

    // ============================================================ state snapshot

    fn publish(&mut self) {
        self.revision += 1;
        let now = Instant::now();
        let local_id = self.identity.device_id();

        let mut peers: Vec<PeerView> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for d in self.trust.list() {
            seen.insert(d.device_id);
            peers.push(self.peer_view(d.device_id, Some(d)));
        }
        for id in self.discovered.keys().chain(self.sessions.keys()).copied().collect::<Vec<_>>() {
            if seen.insert(id) {
                peers.push(self.peer_view(id, None));
            }
        }
        peers.sort_by(|a, b| b.trusted.cmp(&a.trusted).then(b.online.cmp(&a.online)).then(a.name.cmp(&b.name)));

        let routes = self
            .routes
            .iter()
            .map(|r| {
                let rtt = self.sessions.get(&r.peer).map(|s| s.conn.stats().rtt.as_secs_f64() * 1000.0).unwrap_or(0.0);
                let mut stats = RouteStats {
                    codec: match r.profile.as_ref().map(|p| p.codec) {
                        Some(c) if c == Codec::PcmS16Le as u32 => "PCM".into(),
                        Some(_) => "Opus".into(),
                        None => String::new(),
                    },
                    bitrate_kbps: r.bitrate_kbps,
                    loss_pct: r.loss_pct,
                    ..RouteStats::default()
                };
                let frame_ms = r.profile.as_ref().map(|p| p.frame_us as f64 / 1000.0).unwrap_or(10.0);
                if let Some(c) = &r.receiver_controls {
                    stats.buffer_ms = c.buffer_ms.get() as f64;
                    stats.jitter_ms = c.jitter_ms.get() as f64;
                    stats.underruns = c.underruns.load(Ordering::Relaxed);
                    stats.drift_ppm = c.drift_ppm.load(Ordering::Relaxed);
                    stats.level_db = c.level_db.get();
                    stats.latency_ms = stats.buffer_ms + frame_ms + c.device_latency_ms.load(Ordering::Relaxed) as f64 + rtt / 2.0;
                }
                if let Some(c) = &r.sender_controls {
                    stats.level_db = c.level_db.get();
                    if let Some(remote) = &r.remote_stats {
                        stats.buffer_ms = remote.buffer_ms as f64;
                        stats.jitter_ms = remote.jitter_us as f64 / 1000.0;
                        stats.underruns = remote.underruns as u64;
                        stats.drift_ppm = remote.drift_ppm;
                        stats.latency_ms = remote.buffer_ms as f64 + frame_ms * 2.0 + remote.output_latency_ms as f64 + rtt / 2.0;
                    }
                }
                RouteView {
                    route_id: r.key(),
                    peer_id: r.peer.to_hex(),
                    peer_name: self.peer_name(&r.peer),
                    kind: r.kind,
                    status: r.status,
                    started_unix: r.started_unix,
                    elapsed_secs: if r.status == RouteStatus::Active { now.duration_since(r.started).as_secs() } else { 0 },
                    volume: r.volume,
                    muted: r.muted,
                    stats,
                    keep_running: r.keep_running,
                }
            })
            .collect();

        let pairing = PairingView {
            qr_uri: self.qr.as_ref().map(QrPairingPayload::to_uri),
            qr_expires_unix: self.qr.as_ref().map(|q| q.expires_unix).unwrap_or(0),
            prompts: self
                .pairings
                .iter()
                .filter(|(_, p)| !p.qr_scanner && !p.local_confirmed)
                .map(|(id, p)| PairingPrompt {
                    peer_id: id.to_hex(),
                    peer_name: p.hello.device_name.clone(),
                    platform: platform_name(p.hello.platform),
                    code: p.code.clone(),
                    peer_confirmed: p.peer_confirmed,
                })
                .collect(),
        };

        let requests = self
            .requests
            .iter()
            .map(|(id, r)| RouteRequestPrompt {
                request_id: *id,
                peer_id: r.peer.to_hex(),
                peer_name: self.peer_name(&r.peer),
                kind: r.kind,
                expires_unix: r.expires_unix,
            })
            .collect();

        let mic_level_db = self
            .routes
            .iter()
            .filter(|r| r.kind.is_mic())
            .filter_map(|r| r.sender_controls.as_ref().map(|c| c.level_db.get()))
            .fold(-120.0f32, f32::max);

        let state = EngineState {
            revision: self.revision,
            local: LocalDevice {
                device_id: local_id.to_hex(),
                display_code: local_id.display_code(),
                name: self.settings.device_name.clone(),
                platform: self.hooks.platform().to_string(),
                port: self.endpoint.local_port(),
                addresses: local_addresses(self.endpoint.local_port(), false)
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                app_version: self.config.app_version.clone(),
            },
            peers,
            routes,
            pairing,
            requests,
            notices: self.notices.clone(),
            settings: self.settings.clone(),
            capabilities: self.local_capabilities(),
            audio_devices: self.audio_devices.clone(),
            mic_level_db,
            mic_muted: self.mic_muted,
        };
        let _ = self.state_tx.send(Arc::new(state));
    }

    fn peer_view(&self, id: DeviceId, trusted: Option<&TrustedDevice>) -> PeerView {
        let advert = self.discovered.get(&id);
        let session = self.sessions.get(&id);
        let caps = session
            .map(|s| Capabilities(s.hello.capabilities))
            .or_else(|| advert.map(|a| a.capabilities))
            .unwrap_or_default();
        let connection = if session.is_some() {
            ConnectionStatus::Connected
        } else if self.pairings.contains_key(&id) {
            ConnectionStatus::PairingRequired
        } else if let Some(d) = self.dials.get(&id) {
            if d.in_flight {
                ConnectionStatus::Connecting
            } else if d.backoff.in_slow_phase() {
                ConnectionStatus::WaitingForDevice
            } else if trusted.is_some_and(|t| t.auto_connect) {
                ConnectionStatus::Reconnecting
            } else {
                ConnectionStatus::Disconnected
            }
        } else {
            ConnectionStatus::Disconnected
        };
        let (rtt_ms, quality) = match session {
            Some(s) => {
                let st = s.conn.stats();
                let rtt = st.rtt.as_secs_f64() * 1000.0;
                let loss = if st.sent_packets > 0 { st.lost_packets as f64 * 100.0 / st.sent_packets as f64 } else { 0.0 };
                let jitter = self
                    .routes
                    .iter()
                    .filter(|r| r.peer == id)
                    .filter_map(|r| r.receiver_controls.as_ref().map(|c| c.jitter_ms.get() as f64))
                    .fold(0.0, f64::max);
                (rtt, LinkQuality::from_stats(rtt, loss, jitter))
            }
            None => (0.0, LinkQuality::Unknown),
        };
        let name = trusted
            .map(|t| t.display_name().to_string())
            .or_else(|| session.map(|s| s.hello.device_name.clone()))
            .or_else(|| advert.map(|a| a.name.clone()).filter(|n| !n.is_empty()))
            .unwrap_or_else(|| id.display_code());
        let platform = session
            .map(|s| platform_name(s.hello.platform))
            .or_else(|| trusted.map(|t| t.platform.clone()))
            .or_else(|| advert.map(|a| a.platform.clone()))
            .unwrap_or_default();
        PeerView {
            device_id: id.to_hex(),
            name,
            platform,
            trusted: trusted.is_some(),
            online: advert.is_some() || session.is_some(),
            connection,
            quality,
            rtt_ms,
            addresses: advert.map(|a| a.addresses.iter().map(ToString::to_string).collect()).unwrap_or_default(),
            permissions: trusted.map(|t| t.permissions),
            auto_connect: trusted.is_some_and(|t| t.auto_connect),
            blocked: trusted.is_some_and(|t| t.blocked),
            last_seen_unix: trusted.map(|t| t.last_seen_unix).unwrap_or(0),
            can_send_system_audio: caps.has(Capabilities::SOURCE_SYSTEM_AUDIO),
            can_send_app_audio: caps.has(Capabilities::SOURCE_APP_AUDIO),
            can_send_mic: caps.has(Capabilities::SOURCE_MICROPHONE),
            can_play: caps.has(Capabilities::SINK_SPEAKER),
            has_virtual_mic: caps.has(Capabilities::SINK_VIRTUAL_MIC),
        }
    }
}

fn endpoint_info(id: &str, name: &str, kind: EndpointKind) -> EndpointInfo {
    EndpointInfo {
        id: id.into(),
        name: name.into(),
        kind: kind as i32,
        is_default: true,
    }
}

fn new_dial() -> DialState {
    DialState {
        backoff: Backoff::default(),
        next_attempt: Instant::now(),
        in_flight: false,
        task: None,
    }
}

fn new_route(peer: DeviceId, id: u8, kind: RouteKind, requested_locally: bool, volume: f32) -> Route {
    Route {
        peer,
        id,
        kind,
        status: RouteStatus::Starting,
        requested_locally,
        profile: None,
        started: Instant::now(),
        started_unix: now_unix(),
        sender: None,
        receiver: None,
        sender_controls: None,
        receiver_controls: None,
        keep_running: false,
        volume,
        muted: false,
        reply: None,
        remote_stats: None,
        stable_secs: 0,
        clean_secs: 0,
        last_bytes: 0,
        bitrate_kbps: 0,
        last_received: 0,
        last_missing: 0,
        loss_pct: 0.0,
        paused_at: None,
        deadline: None,
    }
}

/// Clamp a peer-supplied profile to safe values.
fn sanitize_profile(p: &mut StreamProfile) {
    p.channels = p.channels.clamp(1, 2);
    p.frame_us = match p.frame_us {
        2_500 | 5_000 | 10_000 | 20_000 => p.frame_us,
        _ => 10_000,
    };
    // PCM frames must fit one datagram: 5 ms stereo or 10 ms mono at 48 kHz.
    if p.codec == Codec::PcmS16Le as u32 {
        p.frame_us = p.frame_us.min(if p.channels == 2 { 5_000 } else { 10_000 });
    }
    if p.codec != Codec::PcmS16Le as u32 && p.codec != Codec::Opus as u32 {
        p.codec = Codec::Opus as u32;
    }
    p.bitrate = p.bitrate.clamp(6_000, 510_000);
    p.jitter_min_ms = p.jitter_min_ms.clamp(5, 500);
    p.jitter_max_ms = p.jitter_max_ms.clamp(p.jitter_min_ms, 1000);
    let _ = MicMode::Default;
}
