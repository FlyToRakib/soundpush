//! The engine actor: owns all state and makes every decision.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use sp_audio_io::{AudioBackend, CaptureSource, DeviceKind, RenderTarget};
use sp_discovery::{Discovery, DiscoveryConfig, DiscoveryEvent, PeerAdvert};
use sp_media::codec::OpusApplication;
use sp_media::profile::{Quality, build_profile};
use sp_protocol::control::{
    ControlMsg, ControlTarget, EndpointInfo, EndpointKind, Hello, MuteSet, NetTestReady,
    NetTestStart, NetTestStop, PairRequest, PairResult, Platform, RouteAccept, RouteReject,
    RouteRequest, RouteStop, RouteUpdate, SessionTicket, StatsReport, StopReason, StreamProfile,
    VolumeSet, control_msg::Body,
};
use sp_protocol::version::LOCAL_VERSIONS;
use sp_protocol::{Capabilities, Codec};
use sp_security::pairing::{
    QrPairingPayload, SAS_EXPORTER_LABEL, pairing_proof, sas_code, verify_pairing_proof,
};
use sp_security::trust::load_or_create_identity;
use sp_security::{
    DeviceId, DeviceIdentity, Fingerprint, PermissionKind, Permissions, Policy, TrustStore,
    TrustedDevice,
};
use sp_transport::{Endpoint, EndpointConfig, SecureConnection, TcpEndpoint, TransportKind};
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, info, warn};

use crate::audit::{AuditEntry, AuditKind, AuditLog, permission_name, policy_name};
use crate::denoise::{DenoiseChange, DenoiseSupervisor};
use crate::error::{ErrorView, Severity, stop_reason_code};
use crate::health::{FALLBACK_BITRATE, LinkHealth, QualityFallback};
use crate::net::{drop_own_addresses, local_addresses, race_candidates, resolve, sort_candidates};
use crate::nettest::NetworkReport;
use crate::pairing_limit::{Decision, PairingLimiter};
use crate::pipeline::monitor::MicMonitor;
use crate::pipeline::receiver::{PacketSink, Receiver, ReceiverConfig};
use crate::pipeline::sender::{Sender, SenderConfig, Subscriber, Subscription};
use crate::pipeline::{EchoReference, ReceiverControls, SenderControls};
use crate::platform::{KeepAlive, PlatformHooks};
use crate::reconnect::{Backoff, FlapDetector};
use crate::resume::ResumeTokens;
use crate::session::{self, Established, SessionCmd, SessionEvent};
use crate::settings::{
    DeviceProfile, LatencyMode, MicMode, SavedRoute, Settings, SettingsStore, TransportPin,
    Visibility,
};
use crate::state::*;
use crate::{EngineConfig, EngineError};

mod local_audio;
mod network_test;
mod pipelines;
mod profiles;

type Reply<T> = oneshot::Sender<Result<T, EngineError>>;

/// At most one state snapshot per interval: UIs re-render (and Android re-parses) each one.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(50);
const LOCAL_ADDRESS_REFRESH: Duration = Duration::from_secs(30);
/// Last-seen times and addresses of trusted devices are written at most this often.
const TRUST_FLUSH_INTERVAL: Duration = Duration::from_secs(30);
/// Network candidates get this head start before the USB (adb reverse) candidate (plan §17.3).
const USB_FALLBACK_DELAY: Duration = Duration::from_secs(2);
/// A single candidate's handshake is given up after this long; the others race on regardless.
const CANDIDATE_TIMEOUT: Duration = Duration::from_secs(3);
/// What packet headers and framing add to a media payload, in percent (plan §19.2 estimate).
const PACKET_OVERHEAD_PCT: u32 = 12;
/// Rough share of one CPU core an encoder group costs: capture, the DSP chain and one encode per
/// frame (`sp-media` benches: ~100 µs per 10 ms stereo Opus frame, so about 1 % of a core).
const ENCODER_CPU_PCT: u32 = 2;
/// Lossless has no encoder, only the copy into the packet.
const PCM_ENCODER_CPU_PCT: u32 = 1;
/// What each receiver of a group adds on top: patching its header and a socket write.
const PER_RECEIVER_CPU_PCT: u32 = 1;
/// Outgoing bandwidth beyond which a shared Wi-Fi link starts to drop audio. Receivers past this
/// are still allowed; the UIs warn before them (plan §19.2).
const SAFE_TOTAL_KBPS: u32 = 8_000;
/// Time to open audio devices once a route is accepted.
const PIPELINE_START_DEADLINE: Duration = Duration::from_secs(20);
/// A blocked device that keeps reconnecting is written to the security log at most this often.
const REFUSED_AUDIT_INTERVAL: Duration = Duration::from_secs(600);
/// Anti-flap (plan §20): a device held on the Stable profile returns to its own latency setting
/// after this long without another reconnect.
const STABLE_HOLD: Duration = Duration::from_secs(600);
/// A feedback loop is reported at most this often, however long it goes on.
const FEEDBACK_NOTICE_INTERVAL: Duration = Duration::from_secs(60);
/// How long the "you have a feedback loop" banner stays after the last howl.
const FEEDBACK_BANNER: Duration = Duration::from_secs(20);

pub(crate) enum Command {
    StartPairing(Reply<String>),
    StopPairing,
    PairWithQr {
        uri: String,
        reply: Reply<()>,
    },
    PairWithDevice {
        device_id: String,
        reply: Reply<()>,
    },
    PairWithAddress {
        address: String,
        reply: Reply<()>,
    },
    ConfirmPairing {
        device_id: String,
        accept: bool,
    },
    Connect {
        device_id: String,
    },
    Disconnect {
        device_id: String,
    },
    ForgetDevice {
        device_id: String,
    },
    SetBlocked {
        device_id: String,
        blocked: bool,
    },
    RenameDevice {
        device_id: String,
        alias: Option<String>,
    },
    SetAutoConnect {
        device_id: String,
        enabled: bool,
    },
    SetPermission {
        device_id: String,
        kind: PermissionKind,
        policy: Policy,
    },
    StartRoute {
        device_id: String,
        kind: RouteKind,
        /// Take the virtual microphone away from whatever feeds it now (plan §8.2).
        replace: bool,
        reply: Reply<String>,
    },
    StopRoute {
        route_id: String,
    },
    SetRouteVolume {
        route_id: String,
        volume: f32,
    },
    SetRouteMuted {
        route_id: String,
        muted: bool,
    },
    SetRouteKeepRunning {
        route_id: String,
        keep: bool,
    },
    SetPeerSpeakersMuted {
        device_id: String,
        muted: bool,
    },
    RespondRouteRequest {
        request_id: u64,
        accept: bool,
        remember: bool,
    },
    SetMicMuted {
        muted: bool,
    },
    SetMicMonitor {
        enabled: bool,
    },
    RefreshAudioDevices,
    AudioDevicesChanged {
        default_input: bool,
        default_output: bool,
    },
    UpdateSettings {
        // Boxed: `Settings` is several times larger than every other command.
        settings: Box<Settings>,
        reply: Reply<Settings>,
    },
    DismissNotice {
        id: u64,
    },
    NetworkChanged,
    SetForeground {
        foreground: bool,
    },
    PreferLossless {
        prefer: bool,
    },
    SetDeviceProfile {
        device_id: String,
        profile: Option<DeviceProfile>,
    },
    RunNetworkTest {
        device_id: String,
        reply: Reply<NetworkReport>,
    },
    CancelNetworkTest {
        device_id: String,
    },
    SimulateConnectionLoss {
        device_id: String,
    },
    AuditLog(Reply<Vec<AuditEntry>>),
    ClearAuditLog(Reply<()>),
    /// `done` is signalled once sessions are closed and everything held has been released.
    Shutdown {
        done: Option<std::sync::mpsc::Sender<()>>,
    },
}

enum Internal {
    Discovery(DiscoveryEvent),
    Incoming(SecureConnection),
    DialFailed {
        conn_id: u64,
        target: Option<DeviceId>,
        error: EngineError,
    },
    Dialed {
        conn_id: u64,
        conn: SecureConnection,
        pair: Option<PairRequest>,
    },
    AudioFailed {
        peer: DeviceId,
        route: u8,
        error: EngineError,
    },
    PairAddressResolved {
        addrs: Vec<SocketAddr>,
        reply: Reply<()>,
    },
    AudioDevicesListed(Vec<sp_audio_io::DeviceInfo>),
    SenderReady {
        key: pipelines::EncoderKey,
        start_id: u64,
        result: Result<Sender, EngineError>,
    },
    ReceiverReady {
        peer: DeviceId,
        route: u8,
        start_id: u64,
        result: Result<(Receiver, PacketSink), EngineError>,
    },
    EncoderFailed {
        key: pipelines::EncoderKey,
        error: EngineError,
    },
    NetTestFinished {
        peer: DeviceId,
        test_id: u32,
        report: NetworkReport,
    },
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
    /// `Degraded` detection, and the path counters it last saw.
    health: LinkHealth,
    path_sent: u64,
    path_lost: u64,
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
    /// Local source: the encoder group this route listens to (see `pipelines`).
    encoder: Option<pipelines::EncoderKey>,
    subscription: Option<Subscription>,
    /// Local sink.
    receiver: Option<Receiver>,
    /// Pipeline being opened off the actor; a result carrying another id is stale.
    start_id: u64,
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
    /// Lossless held on Opus while the link keeps losing packets (plan §8.1); only the route's
    /// requester, which proposes the codec, keeps this.
    quality_fallback: QualityFallback,
    paused_at: Option<Instant>,
    deadline: Option<Instant>,
}

impl Route {
    fn key(&self) -> String {
        route_key(&self.peer, self.id)
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
    /// How loud this device's own speakers are, for the microphone echo duck (plan §15.7).
    echo: Arc<EchoReference>,
    /// Switches noise suppression off while capture cannot keep up (plan §8.3).
    denoise: DenoiseSupervisor,
    /// When a microphone feedback loop was last heard while monitoring (plan §8.2).
    mic_feedback_at: Option<Instant>,
    audio_devices: Vec<AudioDeviceView>,
    keep_alive: KeepAlive,
    foreground: bool,
    /// What "Auto" quality resolves to (plan §14.6). The app sets this when the device is on a
    /// charger with a link that has room for uncompressed audio; desktops leave it alone.
    prefer_lossless: bool,
    peer_speakers_muted: HashMap<DeviceId, bool>,
    /// Peers that muted this device's speakers remotely; unmuted when the last one leaves.
    speakers_muted_by: std::collections::HashSet<DeviceId>,
    /// Capability bits last announced to peers; a change is re-announced mid-session.
    announced_caps: u64,
    /// TLS over TCP: loopback listener on desktops (USB via adb reverse), dial-only elsewhere.
    tcp: Option<Arc<TcpEndpoint>>,
    resume: ResumeTokens,
    encoders: HashMap<pipelines::EncoderKey, pipelines::EncoderSlot>,
    next_start_id: u64,
    net_tests: HashMap<DeviceId, network_test::NetTest>,
    next_test_id: u32,
    /// Cached: listing interfaces on every snapshot was measurable on phones.
    local_addrs: Vec<String>,
    local_addrs_at: Instant,
    last_publish: Instant,
    trust_flushed_at: Instant,
    local_audio: local_audio::LocalAudio,
    /// Anti-flap (plan §20): recent reconnects per device, and devices streaming with the Stable
    /// profile since their last reconnect.
    flaps: HashMap<DeviceId, FlapDetector>,
    stabilized: HashMap<DeviceId, Instant>,
    /// Local security log (plan §21) and the pairing rate limit per source address.
    audit: AuditLog,
    pairing_limiter: PairingLimiter,
    /// When a refused connection from each device was last written to the security log.
    refused_audited: HashMap<DeviceId, Instant>,
}

pub(crate) async fn spawn(
    hooks: Arc<dyn PlatformHooks>,
    config: EngineConfig,
) -> Result<
    (
        mpsc::UnboundedSender<Command>,
        watch::Receiver<Arc<EngineState>>,
    ),
    EngineError,
> {
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
    // Debug logging survives a restart until its 24 hours are up.
    if settings.expire_debug_logging(now_unix()) {
        let _ = settings_store.save(&settings);
    }
    crate::logging::set_debug(settings.debug_logging);

    let endpoint = Arc::new(Endpoint::bind(
        &identity,
        &EndpointConfig {
            preferred_port: config.port,
            ..EndpointConfig::default()
        },
    )?);
    // TLS over TCP on loopback, where `adb reverse` forwards a phone's USB connection (or on
    // every interface while the user pins the TCP transport).
    let tcp = bind_tcp(
        &identity,
        config.tcp_listener,
        tcp_listen_ip(settings.transport),
        endpoint.local_port(),
    )
    .await;

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let (internal_tx, mut internal_rx) = mpsc::unbounded_channel();
    let (session_tx, mut session_rx) = mpsc::unbounded_channel();
    let (state_tx, state_rx) = watch::channel(Arc::new(EngineState::default()));

    let backend = hooks.audio_backend();
    // Advanced override, before any audio thread exists (plan §13.3).
    sp_audio_io::rt_priority::set_enabled(settings.advanced.realtime_audio_priority);
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
        echo: Arc::new(EchoReference::default()),
        denoise: DenoiseSupervisor::default(),
        mic_feedback_at: None,
        audio_devices: Vec::new(),
        keep_alive: KeepAlive::default(),
        foreground: true,
        prefer_lossless: false,
        peer_speakers_muted: HashMap::new(),
        speakers_muted_by: std::collections::HashSet::new(),
        announced_caps: 0,
        tcp: tcp.clone(),
        resume: ResumeTokens::default(),
        encoders: HashMap::new(),
        next_start_id: 0,
        net_tests: HashMap::new(),
        next_test_id: 0,
        local_addrs: Vec::new(),
        local_addrs_at: Instant::now(),
        last_publish: Instant::now(),
        trust_flushed_at: Instant::now(),
        local_audio: local_audio::LocalAudio::default(),
        flaps: HashMap::new(),
        stabilized: HashMap::new(),
        audit: AuditLog::open(&data_dir, now_unix()),
        pairing_limiter: PairingLimiter::default(),
        refused_audited: HashMap::new(),
    };
    actor.refresh_local_addresses();
    if identity_reset {
        actor.notice("notice.identityReset", vec![], Severity::Warning, None);
    }
    if trust_recovered {
        actor.notice(
            "notice.trustStoreRecovered",
            vec![],
            Severity::Warning,
            None,
        );
    }
    if settings_recovered {
        actor.notice("notice.settingsRecovered", vec![], Severity::Warning, None);
    }
    // Reports of panics since the last start, shown once (they stay in diagnostics exports).
    let crashes = crate::crash::take_unseen(&data_dir);
    if crashes > 0 {
        actor.notice(
            "notice.crashReport",
            vec![crashes.to_string()],
            Severity::Warning,
            None,
        );
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

    if let Some(tcp) = tcp {
        spawn_tcp_accept(tcp, internal_tx.clone());
    }

    // Discovery.
    if actor.config.discovery {
        let (discovery, mut events) =
            Discovery::start(actor.identity.clone(), actor.discovery_config());
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
        let mut publish_at: Option<tokio::time::Instant> = None;
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
                _ = tokio::time::sleep_until(publish_at.unwrap_or_else(tokio::time::Instant::now)), if publish_at.is_some() => {}
                else => break,
            }
            publish_at = actor.publish_throttled();
        }
        info!("engine stopped");
    });

    Ok((cmd_tx, state_rx))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Media bandwidth one stream with this profile needs, in kb/s, headers included (plan §19.2).
fn profile_kbps(p: &StreamProfile) -> u32 {
    // 48 kHz × 16 bit × channels for PCM; the negotiated bitrate for Opus.
    let media = if p.codec == Codec::PcmS16Le as u32 {
        48 * 16 * p.channels.clamp(1, 2)
    } else {
        p.bitrate / 1000
    };
    // Redundancy repeats the previous frame in every packet.
    let media = if p.redundancy { media * 2 } else { media };
    media + media * PACKET_OVERHEAD_PCT / 100
}

fn encoder_cpu_pct(p: &StreamProfile) -> u32 {
    if p.codec == Codec::PcmS16Le as u32 {
        PCM_ENCODER_CPU_PCT
    } else {
        ENCODER_CPU_PCT
    }
}

/// Where the TLS-over-TCP listener binds (plan §16.1): loopback, where `adb reverse` delivers a
/// phone's USB connection, or every interface while the user pins the TCP transport, so that a
/// peer pinned the same way can reach this device on a network that blocks UDP. The listener is
/// mutually authenticated like the QUIC one, so a wider bind exposes nothing new.
fn tcp_listen_ip(pin: TransportPin) -> IpAddr {
    match pin {
        TransportPin::Tcp => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        _ => IpAddr::V4(Ipv4Addr::LOCALHOST),
    }
}

/// Bind the TLS-over-TCP endpoint, falling back to a dial-only one when it cannot listen.
async fn bind_tcp(
    identity: &DeviceIdentity,
    listen: bool,
    ip: IpAddr,
    port: u16,
) -> Option<Arc<TcpEndpoint>> {
    if listen {
        match TcpEndpoint::bind(identity, ip, port).await {
            Ok(t) => return Some(Arc::new(t)),
            Err(e) => warn!(error = %e, "no TCP listener: USB connections unavailable"),
        }
    }
    TcpEndpoint::dialer(identity).ok().map(Arc::new)
}

fn spawn_tcp_accept(tcp: Arc<TcpEndpoint>, tx: mpsc::UnboundedSender<Internal>) {
    if tcp.local_port() == 0 {
        return;
    }
    tokio::spawn(async move {
        while let Some(handshake) = tcp.accept().await {
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
        let system_audio = self.backend.supports_loopback();
        LocalCapabilities {
            system_audio,
            app_audio: self.hooks.app_audio_source().is_some(),
            microphone: true,
            // Both halves have to exist for the mixed source (plan §5.1).
            mixed: system_audio,
            speaker: true,
            virtual_mic: virtual_mic.is_some(),
            virtual_mic_input: virtual_mic_device
                .as_deref()
                .and_then(|d| self.hooks.virtual_cable_input(d)),
            virtual_mic_device,
        }
    }

    fn capability_bits(&self) -> Capabilities {
        let caps = self.local_capabilities();
        let mut bits = Capabilities::default()
            .with(Capabilities::CODEC_OPUS)
            .with(Capabilities::CODEC_PCM)
            .with(Capabilities::FEATURE_REMOTE_CONTROL)
            .with(Capabilities::FEATURE_MIC_MONITOR)
            .with(Capabilities::FEATURE_SESSION_RESUME)
            .with(Capabilities::FEATURE_NETWORK_TEST)
            .with(Capabilities::FEATURE_ROUTE_RECONFIGURE)
            .with(Capabilities::FEATURE_DTX)
            .with(Capabilities::FEATURE_RECEIVER_DENOISE);
        if self.tcp.as_ref().is_some_and(|t| t.local_port() != 0) {
            bits = bits.with(Capabilities::TRANSPORT_TCP);
        }
        if caps.system_audio {
            bits = bits.with(Capabilities::SOURCE_SYSTEM_AUDIO);
        }
        if caps.app_audio {
            bits = bits.with(Capabilities::SOURCE_APP_AUDIO);
        }
        if caps.microphone {
            bits = bits.with(Capabilities::SOURCE_MICROPHONE);
        }
        if caps.mixed {
            bits = bits.with(Capabilities::SOURCE_MIXED);
        }
        if caps.speaker {
            bits = bits.with(Capabilities::SINK_SPEAKER);
        }
        if caps.virtual_mic {
            bits = bits.with(Capabilities::SINK_VIRTUAL_MIC);
        }
        bits
    }

    /// `peer`: the trusted device this Hello is for, when known; its resume token is included.
    fn local_hello(&self, peer: Option<DeviceId>) -> Hello {
        let caps = self.local_capabilities();
        let mut endpoints = vec![
            endpoint_info("mic", "Microphone", EndpointKind::SourceMicrophone),
            endpoint_info("speaker", "Speakers", EndpointKind::SinkSpeaker),
        ];
        if caps.system_audio {
            endpoints.push(endpoint_info(
                "system",
                "System audio",
                EndpointKind::SourceSystemAudio,
            ));
        }
        if caps.app_audio {
            endpoints.push(endpoint_info(
                "apps",
                "App audio",
                EndpointKind::SourceAppAudio,
            ));
        }
        if caps.mixed {
            endpoints.push(endpoint_info(
                "mixed",
                "System audio + microphone",
                EndpointKind::SourceMixed,
            ));
        }
        if caps.virtual_mic {
            endpoints.push(endpoint_info(
                "virtual-mic",
                "SoundPush Microphone",
                EndpointKind::SinkVirtualMic,
            ));
        }
        Hello {
            protocol_min: LOCAL_VERSIONS.min.to_u32(),
            protocol_max: LOCAL_VERSIONS.max.to_u32(),
            app_version: self.config.app_version.clone(),
            device_id: Bytes::copy_from_slice(&self.identity.device_id().0),
            device_name: self.settings.device_name.clone(),
            platform: platform_enum(self.hooks.platform()) as i32,
            capabilities: self.capability_bits().0,
            resume_token: peer.and_then(|p| self.resume.held_for(&p, Instant::now())),
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

    fn notice(
        &mut self,
        key: &str,
        args: Vec<String>,
        severity: Severity,
        error: Option<&EngineError>,
    ) {
        let id = self.next_notice_id;
        self.next_notice_id += 1;
        // The support code goes into the log as well, so a report quoting it can be found there
        // even when the user dismissed the message (plan §36.4).
        match error {
            Some(e) => warn!(code = e.code(), key = e.key(), detail = %e, "engine error"),
            None => debug!(key, "notice"),
        }
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
        if matches!(
            error,
            EngineError::Unreachable | EngineError::NetworkBlocked
        ) && self.hooks.inbound_blocked()
        {
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

    /// Publish now, or say when: a burst of events yields at most one snapshot per interval.
    fn publish_throttled(&mut self) -> Option<tokio::time::Instant> {
        let due = self.last_publish + PUBLISH_INTERVAL;
        if Instant::now() >= due {
            self.publish();
            None
        } else {
            Some(tokio::time::Instant::from_std(due))
        }
    }

    fn refresh_local_addresses(&mut self) {
        self.local_addrs = local_addresses(self.endpoint.local_port(), false)
            .iter()
            .map(ToString::to_string)
            .collect();
        self.local_addrs_at = Instant::now();
    }

    /// The addresses this device answers on, for candidates that must not be dialled. The TCP
    /// listener is included: pinned to TCP it answers on every interface, on its own port.
    fn own_addresses(&self) -> Vec<SocketAddr> {
        let tcp_port = self.tcp.as_ref().map(|t| t.local_port()).unwrap_or(0);
        self.local_addrs
            .iter()
            .filter_map(|a| a.parse::<SocketAddr>().ok())
            .flat_map(|a| {
                let tcp = (tcp_port != 0).then(|| SocketAddr::new(a.ip(), tcp_port));
                std::iter::once(a).chain(tcp)
            })
            .collect()
    }

    fn flush_trust(&mut self) {
        if let Err(e) = self.trust.flush() {
            warn!(error = %e, "could not save paired device details");
        }
        self.trust_flushed_at = Instant::now();
    }

    /// Phones dial a computer's `adb reverse` forward on loopback (USB): the port to use, if any.
    fn usb_port(&self, peer_platform: Option<&str>, addrs: &[SocketAddr]) -> Option<u16> {
        let desktop_peer = peer_platform.is_none_or(|p| matches!(p, "windows" | "macos" | "linux"));
        if self.hooks.platform() != "android" || !desktop_peer {
            return None;
        }
        Some(
            addrs
                .iter()
                .map(|a| a.port())
                .find(|p| *p != 0)
                .unwrap_or(sp_transport::DEFAULT_PORT),
        )
    }

    /// "Resume streams after restart" changed: save or drop entries for routes started here.
    fn sync_resumable_routes(&mut self) {
        if self.settings.resume_routes_on_start {
            let active: Vec<(String, RouteKind)> = self
                .routes
                .iter()
                .filter(|r| r.requested_locally && r.status == RouteStatus::Active)
                .map(|r| (r.peer.to_hex(), r.kind))
                .collect();
            for (peer_id, kind) in active {
                if !self
                    .settings
                    .saved_routes
                    .iter()
                    .any(|s| s.matches(&peer_id, kind))
                {
                    self.settings.saved_routes.push(SavedRoute {
                        peer_id,
                        kind,
                        keep: false,
                    });
                }
            }
            self.settings.saved_routes.truncate(32);
        } else {
            self.settings.saved_routes.retain(|s| s.keep);
        }
        self.save_settings();
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
                    let advert = self
                        .discovered
                        .get(&id)
                        .ok_or(EngineError::DeviceNotFound)?;
                    let addrs = advert.addresses.clone();
                    let conn_id = self.dial(
                        addrs,
                        None,
                        Some(PairRequest { qr_proof: None }),
                        None,
                        None,
                    );
                    self.pair_dials.insert(conn_id, false);
                    Ok(())
                });
                let _ = reply.send(result);
            }
            Command::PairWithAddress { address, reply } => {
                // Resolving a hostname can take seconds: never on the actor.
                let tx = self.internal_tx.clone();
                tokio::spawn(async move {
                    let addrs = resolve(&address, sp_transport::DEFAULT_PORT).await;
                    let _ = tx.send(Internal::PairAddressResolved { addrs, reply });
                });
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
                    self.forget_speaker_mutes(id);
                    self.remove_routes_for(id);
                    self.end_network_test(id, EngineError::Unreachable);
                    self.flaps.remove(&id);
                    self.stabilized.remove(&id);
                    // Suppress auto reconnect for this device until the user connects again.
                    let _ = self.trust.update(&id, |d| d.auto_connect = false);
                }
            }
            Command::ForgetDevice { device_id } => {
                if let Ok(id) = parse_device(&device_id) {
                    // Logged after its routes stop, while the name is still known.
                    self.revoke(id);
                    if self.trust.get(&id).is_some() {
                        self.audit_peer(AuditKind::DeviceForgotten, &id, None, "");
                    }
                    let _ = self.trust.remove(&id);
                    self.forget_network_test(&id);
                    let peer_id = id.to_hex();
                    let had_profile = self.settings.device_profiles.remove(&peer_id).is_some();
                    let saved = self.settings.saved_routes.len();
                    self.settings.saved_routes.retain(|s| s.peer_id != peer_id);
                    if had_profile || saved != self.settings.saved_routes.len() {
                        self.save_settings();
                    }
                }
            }
            Command::SetBlocked { device_id, blocked } => {
                if let Ok(id) = parse_device(&device_id) {
                    let _ = self.trust.update(&id, |d| d.blocked = blocked);
                    if blocked {
                        self.revoke(id);
                    }
                    if self.trust.get(&id).is_some() {
                        let kind = if blocked {
                            AuditKind::DeviceBlocked
                        } else {
                            AuditKind::DeviceUnblocked
                        };
                        self.audit_peer(kind, &id, None, "");
                    }
                }
            }
            Command::RenameDevice { device_id, alias } => {
                if let Ok(id) = parse_device(&device_id) {
                    let alias = alias
                        .map(|a| a.trim().chars().take(64).collect::<String>())
                        .filter(|a| !a.is_empty());
                    let _ = self.trust.update(&id, |d| d.alias = alias);
                }
            }
            Command::SetAutoConnect { device_id, enabled } => {
                if let Ok(id) = parse_device(&device_id) {
                    let _ = self.trust.update(&id, |d| d.auto_connect = enabled);
                }
            }
            Command::SetPermission {
                device_id,
                kind,
                policy,
            } => {
                if let Ok(id) = parse_device(&device_id) {
                    let _ = self.trust.update(&id, |d| d.permissions.set(kind, policy));
                    if self.trust.get(&id).is_some() {
                        self.audit_permission(&id, kind, policy);
                    }
                    // Revocation takes effect immediately.
                    if policy == Policy::Deny {
                        self.enforce_permissions(id);
                    }
                }
            }
            Command::StartRoute {
                device_id,
                kind,
                replace,
                reply,
            } => match parse_device(&device_id) {
                Ok(id) => self.start_route(id, kind, replace, reply),
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            },
            Command::StopRoute { route_id } => {
                self.stop_route_by_key(&route_id, StopReason::UserStopped, true)
            }
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
                    let (peer_id, kind) = (r.peer.to_hex(), r.kind);
                    let auto = r.requested_locally && self.settings.resume_routes_on_start;
                    self.settings
                        .saved_routes
                        .retain(|s| !s.matches(&peer_id, kind));
                    if keep || auto {
                        self.settings.saved_routes.push(SavedRoute {
                            peer_id,
                            kind,
                            keep,
                        });
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
            Command::UpdateSettings { settings, reply } => {
                let mut settings = *settings;
                settings.sanitize();
                if settings.device_name.is_empty() {
                    settings.device_name = self.settings.device_name.clone();
                }
                settings.schedule_debug_logging(self.settings.debug_logging, now_unix());
                crate::logging::set_debug(settings.debug_logging);
                let old = std::mem::replace(&mut self.settings, settings);
                self.save_settings();
                self.apply_settings(&old);
                // Pinning (or unpinning) TCP moves the listener between loopback and every
                // interface. Open connections are unaffected.
                if tcp_listen_ip(old.transport) != tcp_listen_ip(self.settings.transport) {
                    self.rebind_tcp().await;
                }
                let _ = reply.send(Ok(self.settings.clone()));
            }
            Command::DismissNotice { id } => self.notices.retain(|n| n.id != id),
            Command::NetworkChanged => {
                for d in self.dials.values_mut() {
                    d.backoff.reset();
                    d.next_attempt = Instant::now();
                }
                self.refresh_local_addresses();
                self.update_discovery();
            }
            Command::SetForeground { foreground } => {
                self.foreground = foreground;
                // A backgrounded phone app may be killed without warning.
                if !foreground {
                    self.flush_trust();
                }
            }
            Command::PreferLossless { prefer } => {
                if self.prefer_lossless != prefer {
                    self.prefer_lossless = prefer;
                    // "Auto" resolves differently now; re-apply it to the routes this device
                    // proposed the codec for. Routes on a fixed quality are left alone.
                    let keys: Vec<String> = self
                        .routes
                        .iter()
                        .filter(|r| r.status == RouteStatus::Active && r.requested_locally)
                        .map(Route::key)
                        .collect();
                    for key in keys {
                        self.reconfigure_route(&key);
                    }
                }
            }
            Command::SetDeviceProfile { device_id, profile } => {
                if let Ok(id) = parse_device(&device_id) {
                    let old = self.settings.clone();
                    match profile.filter(|p| !p.is_empty()) {
                        Some(p) => {
                            self.settings.device_profiles.insert(id.to_hex(), p);
                        }
                        None => {
                            self.settings.device_profiles.remove(&id.to_hex());
                        }
                    }
                    self.settings.sanitize();
                    self.save_settings();
                    self.apply_settings(&old);
                }
            }
            Command::RunNetworkTest { device_id, reply } => match parse_device(&device_id) {
                Ok(id) => self.start_network_test(id, reply),
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            },
            Command::CancelNetworkTest { device_id } => {
                if let Ok(id) = parse_device(&device_id) {
                    self.cancel_network_test(id);
                }
            }
            Command::SimulateConnectionLoss { device_id } => {
                if let Some(s) = parse_device(&device_id)
                    .ok()
                    .and_then(|id| self.sessions.get(&id))
                {
                    s.conn.close(0, b"simulated loss");
                }
            }
            Command::AuditLog(reply) => {
                let _ = reply.send(Ok(self.audit.entries()));
            }
            Command::ClearAuditLog(reply) => {
                let result = self
                    .audit
                    .clear(now_unix())
                    .map_err(|e| EngineError::Storage(e.to_string()));
                let _ = reply.send(result);
            }
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
        // A new transport pin should be tried at once, not after the current backoff.
        if old.transport != self.settings.transport {
            for d in self.dials.values_mut() {
                d.backoff.reset();
                d.next_attempt = Instant::now();
            }
        }
        // Live updates to running routes.
        for r in &self.routes {
            if let Some(c) = r.receiver_controls.as_ref().filter(|_| !r.kind.is_mic()) {
                c.balance.set(self.settings.output.balance);
                c.mono.store(self.settings.output.mono, Ordering::Relaxed);
                c.av_offset_ms
                    .store(self.settings.output.av_offset_ms, Ordering::Relaxed);
            }
        }
        // Switching noise suppression off clears a suspension, so turning it on again really does.
        if old.mic.noise_suppression != self.settings.mic.noise_suppression {
            self.denoise.reset();
        }
        self.update_mic_groups();
        // "Mute this computer's speakers while sending" also applies to a stream already running.
        if old.capture.mute_local_speakers != self.settings.capture.mute_local_speakers
            && self.speakers_muted_by.is_empty()
            && self
                .routes
                .iter()
                .any(|r| r.kind == RouteKind::SendSystemAudio && r.sender_controls.is_some())
        {
            self.hooks
                .set_speakers_muted(self.settings.capture.mute_local_speakers);
        }
        // Latency, quality and redundancy, global or per device.
        self.reconfigure_routes(old);
        if old.resume_routes_on_start != self.settings.resume_routes_on_start {
            self.sync_resumable_routes();
        }
        if old.mic.monitor != self.settings.mic.monitor {
            self.set_monitor(self.settings.mic.monitor);
        } else if let Some(m) = &self.monitor {
            m.gain_db.set(self.settings.mic.gain_db);
        }
        // Advanced overrides. Real-time priority applies to audio threads started from now on;
        // "keep audio devices open" and "continuous capture" are read where they are used.
        sp_audio_io::rt_priority::set_enabled(self.settings.advanced.realtime_audio_priority);
    }

    /// Move the TLS-over-TCP listener after a transport pin change (plan §16.1). Sessions already
    /// running over TCP keep their connections; only the listener is replaced. `TRANSPORT_TCP`
    /// may change with it, which `tick` re-announces to connected peers.
    async fn rebind_tcp(&mut self) {
        if let Some(old) = self.tcp.take() {
            old.close();
        }
        let tcp = bind_tcp(
            &self.identity,
            self.config.tcp_listener,
            tcp_listen_ip(self.settings.transport),
            self.endpoint.local_port(),
        )
        .await;
        self.tcp = tcp.clone();
        if let Some(tcp) = tcp {
            spawn_tcp_accept(tcp, self.internal_tx.clone());
        }
        self.update_discovery();
    }

    fn refresh_audio_devices(&mut self) {
        // Listing devices can block for seconds (Bluetooth, sleeping USB interfaces): off the actor.
        let backend = self.backend.clone();
        let hooks = self.hooks.clone();
        let tx = self.internal_tx.clone();
        tokio::task::spawn_blocking(move || {
            hooks.audio_devices_changed();
            // A broken audio stack must never stop the engine from starting.
            let listed =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| backend.list_devices()));
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
            let _ = tx.send(Internal::AudioDevicesListed(devices));
        });
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

    /// Race the candidates (plan §17.3). Returns the conn_id the resulting session will use.
    /// With `usb_port`, a TLS-over-TCP attempt to that loopback port (an `adb reverse` forward)
    /// races the network candidates, starting after [`USB_FALLBACK_DELAY`] if there are any.
    fn dial(
        &mut self,
        mut addrs: Vec<SocketAddr>,
        pinned: Option<DeviceId>,
        pair: Option<PairRequest>,
        target: Option<DeviceId>,
        usb_port: Option<u16>,
    ) -> u64 {
        sort_candidates(&mut addrs, self.config.include_loopback);
        // An address this computer answers on is never the peer (see `drop_own_addresses`).
        drop_own_addresses(&mut addrs, &self.own_addresses());
        // The pinned transport (plan §16.1) decides which candidates exist at all; Auto keeps
        // both, with the USB head start.
        let pin = self.settings.transport;
        let over_tcp = pin == TransportPin::Tcp;
        if matches!(pin, TransportPin::Usb) {
            addrs.clear();
        }
        let usb_port = usb_port.filter(|_| pin != TransportPin::Quic);
        let conn_id = self.next_conn_id();
        let endpoint = self.endpoint.clone();
        // `Some` on the network candidates only while TCP is pinned; the USB candidate is
        // always TCP.
        let network_tcp = self.tcp.clone().filter(|_| over_tcp);
        let usb_tcp = self.tcp.clone().filter(|_| usb_port.is_some());
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            let has_network = !addrs.is_empty();
            // The ranked candidates race each other, each starting 250 ms after the one before
            // it; the first authenticated handshake wins and the rest are cancelled.
            let network = race_candidates(addrs, move |addr| {
                let endpoint = endpoint.clone();
                let tcp = network_tcp.clone();
                async move {
                    let connect = async {
                        match &tcp {
                            Some(tcp) => tcp.connect(addr, pinned).await,
                            None => endpoint.connect(addr, pinned).await,
                        }
                    };
                    match tokio::time::timeout(CANDIDATE_TIMEOUT, connect).await {
                        Ok(Ok(conn)) => Ok(conn),
                        Ok(Err(e)) => {
                            debug!(%addr, error = %e, "dial failed");
                            Err(EngineError::from(e))
                        }
                        Err(_) => Err(EngineError::Unreachable),
                    }
                }
            });
            let usb = async move {
                let (Some(tcp), Some(port)) = (usb_tcp, usb_port) else {
                    return Err(EngineError::Unreachable);
                };
                // Head start for the network only when both transports are in play: pinned to
                // TCP, the USB candidate is no slower than the others.
                if has_network && !over_tcp {
                    tokio::time::sleep(USB_FALLBACK_DELAY).await;
                }
                tcp.connect(SocketAddr::from(([127, 0, 0, 1], port)), pinned)
                    .await
                    .map_err(EngineError::from)
            };
            match first_success(network, usb).await {
                Ok(conn) => {
                    let _ = tx.send(Internal::Dialed {
                        conn_id,
                        conn,
                        pair,
                    });
                }
                Err(error) => {
                    let _ = tx.send(Internal::DialFailed {
                        conn_id,
                        target,
                        error,
                    });
                }
            }
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
        let mut addrs: Vec<SocketAddr> = self
            .discovered
            .get(&id)
            .map(|a| a.addresses.clone())
            .unwrap_or_default();
        addrs.extend(
            device
                .last_addresses
                .iter()
                .filter_map(|a| a.parse::<SocketAddr>().ok()),
        );
        let usb_port = self.usb_port(Some(&device.platform), &addrs);
        let state = self.dials.entry(id).or_insert_with(new_dial);
        if state.in_flight || Instant::now() < state.next_attempt {
            return;
        }
        if addrs.is_empty() && usb_port.is_none() {
            let delay = state.backoff.next_delay();
            state.next_attempt = Instant::now() + delay;
            return;
        }
        state.in_flight = true;
        // Pinned by device ID during the handshake; on_established then requires the exact stored key.
        self.dial(addrs, Some(id), None, Some(id), usb_port);
    }

    fn pair_with_qr(&mut self, uri: &str) -> Result<(), EngineError> {
        let payload = QrPairingPayload::from_uri(uri)
            .map_err(|_| EngineError::InvalidInput("pairing code".into()))?;
        // Expiry is enforced by the displaying device, which rejects stale secrets.
        if payload.device_id == self.identity.device_id() {
            return Err(EngineError::InvalidInput(
                "this is your own pairing code".into(),
            ));
        }
        let proof = pairing_proof(
            &payload.secret,
            &self.identity.fingerprint(),
            &payload.device_id,
        );
        // Pinned to the code's device ID, so the USB candidate cannot reach anyone else.
        let usb_port = self.usb_port(None, &payload.addresses);
        let conn_id = self.dial(
            payload.addresses.clone(),
            Some(payload.device_id),
            Some(PairRequest {
                qr_proof: Some(Bytes::copy_from_slice(&proof)),
            }),
            Some(payload.device_id),
            usb_port,
        );
        self.pair_dials.insert(conn_id, true);
        Ok(())
    }

    // ============================================================ internal events

    fn handle_internal(&mut self, ev: Internal) {
        match ev {
            Internal::Incoming(conn) => {
                let conn_id = self.next_conn_id();
                let hello = self.local_hello(self.trusted_peer_of(&conn));
                tokio::spawn(session::run(
                    conn,
                    false,
                    conn_id,
                    hello,
                    None,
                    self.session_tx.clone(),
                ));
            }
            Internal::Dialed {
                conn_id,
                conn,
                pair,
            } => {
                let hello = self.local_hello(self.trusted_peer_of(&conn));
                tokio::spawn(session::run(
                    conn,
                    true,
                    conn_id,
                    hello,
                    pair,
                    self.session_tx.clone(),
                ));
            }
            Internal::DialFailed {
                conn_id,
                target,
                error,
            } => {
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
            Internal::PairAddressResolved { addrs, reply } => {
                let result = if addrs.is_empty() {
                    Err(EngineError::InvalidInput("address".into()))
                } else {
                    // On a phone, a loopback address means a USB forward to a computer.
                    let usb_port = if self.hooks.platform() == "android" {
                        addrs
                            .iter()
                            .find(|a| a.ip().is_loopback())
                            .map(|a| a.port())
                    } else {
                        None
                    };
                    let conn_id = self.dial(
                        addrs,
                        None,
                        Some(PairRequest { qr_proof: None }),
                        None,
                        usb_port,
                    );
                    self.pair_dials.insert(conn_id, false);
                    Ok(())
                };
                let _ = reply.send(result);
            }
            Internal::AudioDevicesListed(devices) => {
                let hooks = self.hooks.clone();
                self.audio_devices = devices
                    .into_iter()
                    .map(|d| AudioDeviceView {
                        virtual_cable: d.kind == DeviceKind::Output
                            && hooks.virtual_cable_input(&d.name).is_some(),
                        id: d.id,
                        name: d.name,
                        is_input: d.kind == DeviceKind::Input,
                        is_default: d.is_default,
                    })
                    .collect();
            }
            Internal::SenderReady {
                key,
                start_id,
                result,
            } => self.on_sender_ready(key, start_id, result),
            Internal::ReceiverReady {
                peer,
                route,
                start_id,
                result,
            } => self.on_receiver_ready(peer, route, start_id, result),
            Internal::EncoderFailed { key, error } => self.on_encoder_failed(key, error),
            Internal::NetTestFinished {
                peer,
                test_id,
                report,
            } => self.on_net_test_finished(peer, test_id, report),
        }
    }

    /// The device behind an authenticated connection, if it is trusted.
    fn trusted_peer_of(&self, conn: &SecureConnection) -> Option<DeviceId> {
        let id = conn.peer_fingerprint().device_id();
        self.trust.get(&id).map(|_| id)
    }

    fn handle_session(&mut self, ev: SessionEvent) {
        match ev {
            SessionEvent::Established(est) => self.on_established(est),
            SessionEvent::Control { conn_id, msg } => self.on_control(conn_id, msg),
            SessionEvent::Closed { conn_id, reason } => self.on_closed(conn_id, reason),
            SessionEvent::HandshakeFailed {
                conn_id,
                incompatible,
            } => {
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
        // Freed whatever the peer turns out to be: a pairing dial can reach an already trusted device.
        let pair_dial = self.pair_dials.remove(&est.conn_id);
        let key = est.conn.peer_public_key();
        let id = Fingerprint::of_public_key(&key).device_id();

        if let Some(device) = self.trust.get(&id).cloned() {
            if device.blocked || device.public_key != key {
                let _ = est.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
                self.audit_refused(
                    id,
                    if device.blocked {
                        "blocked"
                    } else {
                        "keyChanged"
                    },
                );
                return;
            }
            // Session resume: looked at only after authentication and the trust check (resume.rs).
            if let Some(token) = &est.hello.resume_token {
                if self.resume.redeem(&key, token, Instant::now()) {
                    info!(peer = %id.short(), "session resumed");
                }
            }
            self.promote_session(id, est);
            return;
        }

        // Untrusted peer.
        if est.dialed {
            match pair_dial {
                // We dialed to pair; the session already sent our PairRequest.
                Some(qr_scanner) => self.begin_pairing(id, est, qr_scanner),
                // A dial that was not for pairing reached an untrusted key (e.g. device was forgotten).
                None => {
                    let _ = est.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
                }
            }
        } else {
            // Pairing rate limit per source address (plan §21.1), before any pairing work.
            let address = est.conn.remote_address().ip().to_canonical();
            if let Decision::Refused { first } = self.pairing_limiter.check(address, Instant::now())
            {
                let _ = est.tx.send(SessionCmd::Close(StopReason::RateLimited));
                if first {
                    warn!(%address, "too many pairing attempts; refusing this address for a minute");
                    self.audit.record(
                        AuditEntry::new(now_unix(), AuditKind::PairingRateLimited)
                            .peer(&est.hello.device_name, id.display_code())
                            .detail(address.to_string()),
                    );
                    self.notice(
                        "notice.pairingRateLimited",
                        vec![address.to_string()],
                        Severity::Warning,
                        None,
                    );
                }
                return;
            }
            // Wait for the peer's PairRequest (30 s).
            self.unpaired.insert(est.conn_id, (est, Instant::now()));
        }
    }

    /// Write a security event about `peer` (its current name and short code) to the audit log.
    fn audit_peer(
        &mut self,
        kind: AuditKind,
        peer: &DeviceId,
        route: Option<RouteKind>,
        detail: &str,
    ) {
        let mut entry = AuditEntry::new(now_unix(), kind)
            .peer(&self.peer_name(peer), peer.display_code())
            .detail(detail);
        if let Some(route) = route {
            entry = entry.route(route);
        }
        self.audit.record(entry);
    }

    fn audit_permission(&mut self, peer: &DeviceId, kind: PermissionKind, policy: Policy) {
        let detail = format!("{}={}", permission_name(kind), policy_name(policy));
        self.audit_peer(AuditKind::PermissionChanged, peer, None, &detail);
    }

    /// A refused connection from a paired device, logged at most once per
    /// [`REFUSED_AUDIT_INTERVAL`] so a blocked device that keeps retrying cannot flood the log.
    fn audit_refused(&mut self, id: DeviceId, detail: &str) {
        let now = Instant::now();
        if self
            .refused_audited
            .get(&id)
            .is_some_and(|t| now.duration_since(*t) < REFUSED_AUDIT_INTERVAL)
        {
            return;
        }
        self.refused_audited.insert(id, now);
        self.audit_peer(AuditKind::ConnectionRefused, &id, None, detail);
    }

    fn begin_pairing(&mut self, id: DeviceId, est: Established, qr_scanner: bool) {
        // The code both users compare comes from the TLS session; without it there is nothing
        // to verify, so pairing stops rather than showing a predictable code.
        let Ok(exporter) = est.conn.export_keying_material(SAS_EXPORTER_LABEL, b"") else {
            let _ = est.tx.send(SessionCmd::Close(StopReason::Unspecified));
            self.error_notice(
                &EngineError::Internal("pairing code unavailable".into()),
                vec![],
            );
            return;
        };
        let code = sas_code(
            &exporter,
            &self.identity.fingerprint(),
            &est.conn.peer_fingerprint(),
        );
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
        let _ = p.tx.send(SessionCmd::Send(ControlMsg::new(
            0,
            Body::PairResult(PairResult { accepted: accept }),
        )));
        if !accept {
            self.audit_peer(AuditKind::PairingRejected, &id, None, "local");
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
        self.audit.record(
            AuditEntry::new(now_unix(), AuditKind::PairingSucceeded).peer(&name, id.display_code()),
        );
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
            if old_preferred
                && !new_preferred
                && old.connected_at.elapsed() < Duration::from_secs(5)
            {
                let _ = est.tx.send(SessionCmd::Close(StopReason::Superseded));
                self.sessions.insert(id, old);
                // The rejected connection may be our own dial: free its dial state.
                self.cancel_dial(&id);
                return;
            }
            self.conn_index.remove(&old.conn_id);
            let _ = old.tx.send(SessionCmd::Close(StopReason::Superseded));
            self.pause_routes_for(id);
            self.end_network_test(id, EngineError::Unreachable);
        }
        // Hints only: written by the periodic flush, not on every connection.
        self.trust.update_hints(&id, |d| {
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
        let path = est.conn.stats();
        let session = Session {
            conn_id: est.conn_id,
            conn: est.conn,
            hello: est.hello,
            dialed: est.dialed,
            tx: est.tx,
            next_route: if est.dialed { 2 } else { 1 },
            connected_at: Instant::now(),
            health: LinkHealth::default(),
            path_sent: path.sent_packets,
            path_lost: path.lost_packets,
        };
        // A token to resume this session after a network interruption (plan §20).
        if Capabilities(session.hello.capabilities).has(Capabilities::FEATURE_SESSION_RESUME) {
            let token = self
                .resume
                .issue(session.conn.peer_public_key(), Instant::now());
            session.send(Body::SessionTicket(SessionTicket {
                token: Bytes::copy_from_slice(&token),
                lifetime_secs: crate::resume::TOKEN_LIFETIME.as_secs() as u32,
            }));
        }
        self.sessions.insert(id, session);
        self.resume_routes(id);
    }

    fn on_closed(&mut self, conn_id: u64, reason: StopReason) {
        self.unpaired.remove(&conn_id);
        if let Some((id, _)) = self
            .pairings
            .iter()
            .find(|(_, p)| p.conn_id == conn_id)
            .map(|(k, v)| (*k, v.conn_id))
        {
            self.pairings.remove(&id);
            match reason {
                StopReason::PermissionDenied => {
                    self.error_notice(&EngineError::PairingRejected, vec![]);
                }
                StopReason::RateLimited => {
                    self.error_notice(&EngineError::PairingRateLimited, vec![]);
                }
                _ => {}
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
        self.forget_speaker_mutes(id);
        self.end_network_test(id, EngineError::Unreachable);

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
            StopReason::UserStopped => {
                self.remove_routes_for(id);
            }
            // Everything else pauses, including `Superseded`: the connection that replaced this
            // one may already be established (or about to be) and resumes the paused routes.
            _ => {
                self.pause_routes_for(id);
                // A superseded connection was replaced on purpose; it is not a drop.
                if reason != StopReason::Superseded {
                    self.record_reconnect(id);
                }
                if self.trust.get(&id).is_some_and(|d| d.auto_connect) {
                    let state = self.dials.entry(id).or_insert_with(new_dial);
                    state.in_flight = false;
                    state.backoff.reset();
                    state.next_attempt = Instant::now() + Duration::from_millis(300);
                }
            }
        }
    }

    /// Anti-flap (plan §20): more than five drops in two minutes switch the device's routes to
    /// the Stable profile until it has stayed connected for [`STABLE_HOLD`]. Paused routes pick
    /// it up when they resume ([`Self::profile_for`]).
    fn record_reconnect(&mut self, id: DeviceId) {
        let now = Instant::now();
        let flapping = self.flaps.entry(id).or_default().record(now);
        if let Some(since) = self.stabilized.get_mut(&id) {
            *since = now;
            return;
        }
        if flapping {
            self.stabilized.insert(id, now);
            warn!(peer = %id.short(), "connection keeps dropping; switching to the Stable profile");
            let name = self.peer_name(&id);
            self.notice(
                "notice.unstableConnection",
                vec![name],
                Severity::Warning,
                None,
            );
        }
    }

    /// Devices held on the Stable profile that have been connected long enough go back to their
    /// own latency setting; running routes are reconfigured live.
    fn expire_stable_holds(&mut self, now: Instant) {
        let expired: Vec<DeviceId> = self
            .stabilized
            .iter()
            .filter(|(_, since)| now.duration_since(**since) >= STABLE_HOLD)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            self.stabilized.remove(&id);
            self.flaps.remove(&id);
            info!(peer = %id.short(), "connection stable again; restoring its latency profile");
            let keys: Vec<String> = self
                .routes
                .iter()
                .filter(|r| r.peer == id && r.status == RouteStatus::Active)
                .map(Route::key)
                .collect();
            for key in keys {
                self.reconfigure_route(&key);
            }
        }
    }

    /// Once per second: a session is `Degraded` while loss or jitter stays above the threshold
    /// (plan §19.1). Loss is the worse of the QUIC path and the session's routes.
    fn update_link_health(&mut self) {
        for (id, s) in &mut self.sessions {
            let path = s.conn.stats();
            let sent = path.sent_packets.saturating_sub(s.path_sent);
            let lost = path.lost_packets.saturating_sub(s.path_lost);
            s.path_sent = path.sent_packets;
            s.path_lost = path.lost_packets;
            // Keep-alives alone are too few packets to say anything about loss.
            let mut loss = if sent >= 20 {
                lost as f64 * 100.0 / sent as f64
            } else {
                0.0
            };
            let mut jitter = 0.0f64;
            for r in self
                .routes
                .iter()
                .filter(|r| r.peer == *id && r.status == RouteStatus::Active)
            {
                loss = loss.max(r.loss_pct);
                if let Some(c) = &r.receiver_controls {
                    jitter = jitter.max(c.jitter_ms.get() as f64);
                } else if let Some(remote) = &r.remote_stats {
                    jitter = jitter.max(remote.jitter_us as f64 / 1000.0);
                }
            }
            if s.health.update(loss, jitter) {
                info!(peer = %id.short(), degraded = s.health.is_degraded(), loss, jitter, "connection quality changed");
            }
        }
    }

    /// Lossless (PCM) streams that keep losing packets fall back to Opus, and go back to lossless
    /// when the link is good again (plan §8.1, §15.6). Both ways are announced, because the user
    /// asked for lossless and would otherwise wonder where it went.
    ///
    /// Only the route's requester proposes the codec, so only it runs this; the other side
    /// receives the change as an ordinary `RouteUpdate`.
    fn update_quality_fallback(&mut self) {
        let pcm = Codec::PcmS16Le as u32;
        let changed: Vec<(String, DeviceId, bool)> = self
            .routes
            .iter_mut()
            .filter(|r| r.requested_locally && r.status == RouteStatus::Active)
            .filter(|r| {
                r.quality_fallback.is_active() || r.profile.as_ref().is_some_and(|p| p.codec == pcm)
            })
            .filter_map(|r| {
                r.quality_fallback
                    .update(r.loss_pct)
                    .then(|| (r.key(), r.peer, r.quality_fallback.is_active()))
            })
            .collect();
        for (key, peer, active) in changed {
            self.reconfigure_route(&key);
            let name = self.peer_name(&peer);
            if active {
                info!(route = key, "lossless fell back to Opus");
                self.notice(
                    "notice.qualityFallback",
                    vec![name],
                    Severity::Warning,
                    None,
                );
            } else {
                info!(route = key, "lossless restored");
                self.notice("notice.qualityRestored", vec![name], Severity::Info, None);
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
        if let Some(id) = self
            .pairings
            .iter()
            .find(|(_, p)| p.conn_id == conn_id)
            .map(|(k, _)| *k)
        {
            if let Body::PairResult(result) = body {
                let Some(p) = self.pairings.get_mut(&id) else {
                    return;
                };
                if !result.accepted {
                    self.audit_peer(AuditKind::PairingRejected, &id, None, "peer");
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
                        // A device answers `DeviceBusy` when its source already feeds as many
                        // receivers as it allows (plan §19.2).
                        StopReason::DeviceBusy => EngineError::TooManyReceivers,
                        // Another device already feeds the peer's virtual microphone; the app
                        // offers to replace it (plan §8.2).
                        StopReason::VirtualMicBusy => EngineError::VirtualMicBusy,
                        StopReason::AudioDeviceLost => EngineError::AudioDevice("remote".into()),
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
            Body::RouteUpdate(update) => self.on_route_update(peer, update),
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
            Body::SessionTicket(ticket) => {
                if self
                    .sessions
                    .get(&peer)
                    .is_some_and(|s| s.conn_id == conn_id)
                {
                    self.resume
                        .hold(peer, ticket.token, ticket.lifetime_secs, Instant::now());
                }
            }
            Body::NetTestStart(start) => self.on_net_test_start(peer, start),
            Body::NetTestReady(ready) => self.on_net_test_ready(peer, ready),
            Body::NetTestStop(stop) => self.on_net_test_stop(peer, stop),
        }
    }

    fn on_pair_request(&mut self, est: Established, req: PairRequest) {
        let id = est.conn.peer_fingerprint().device_id();
        let entry = |kind| {
            AuditEntry::new(now_unix(), kind).peer(&est.hello.device_name, id.display_code())
        };
        self.audit.record(
            entry(AuditKind::PairingAttempt)
                .detail(est.conn.remote_address().ip().to_canonical().to_string()),
        );
        match (req.qr_proof, self.qr.as_ref()) {
            (Some(proof), Some(qr)) if !qr.is_expired(now_unix()) => {
                let ok = verify_pairing_proof(
                    &qr.secret,
                    &est.conn.peer_fingerprint(),
                    &self.identity.device_id(),
                    &proof,
                )
                .is_ok();
                if !ok {
                    self.audit
                        .record(entry(AuditKind::PairingRejected).detail("proof"));
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
                let _ = tx.send(SessionCmd::Send(ControlMsg::new(
                    0,
                    Body::PairResult(PairResult { accepted: true }),
                )));
                self.complete_pairing(id);
            }
            (None, Some(_)) => {
                // Code pairing is only accepted while pairing mode is open on this device.
                self.begin_pairing(id, est, false);
            }
            (proof, _) => {
                // A QR proof whose code expired, or pairing mode is not open.
                let detail = if proof.is_some() && self.qr.is_some() {
                    "proof"
                } else {
                    "closed"
                };
                self.audit
                    .record(entry(AuditKind::PairingRejected).detail(detail));
                let _ = est.tx.send(SessionCmd::Close(StopReason::PermissionDenied));
            }
        }
    }

    // ============================================================ routes

    fn start_route(
        &mut self,
        peer: DeviceId,
        kind: RouteKind,
        replace: bool,
        reply: Reply<String>,
    ) {
        self.start_route_keeping(peer, kind, replace, reply, QualityFallback::default());
    }

    /// `fallback`: the quality fallback of the route this one replaces, so a route restarted on a
    /// lossy link does not ask for lossless again (`profiles::restart_route`).
    fn start_route_keeping(
        &mut self,
        peer: DeviceId,
        kind: RouteKind,
        replace: bool,
        reply: Reply<String>,
        fallback: QualityFallback,
    ) {
        // Starting a route that already runs is a no-op, answered before anything can refuse it:
        // repeated start calls stay safe (plan §27.2), including at the receiver limit.
        if let Some(existing) = self
            .routes
            .iter()
            .find(|r| r.peer == peer && r.kind == kind && r.status != RouteStatus::Stopped)
        {
            let _ = reply.send(Ok(existing.key()));
            return;
        }
        if let Err(e) = self.check_local_capability(kind) {
            let _ = reply.send(Err(e));
            return;
        }
        // Only one device at a time can be the virtual microphone (plan §15.8). The app asks
        // "Replace current microphone source?" and starts again with `replace`.
        if kind == RouteKind::ReceiveMicToVirtualMic {
            match self.virtual_mic_feed(Some(peer)) {
                Some(key) if replace => {
                    self.stop_route_by_key(&key, StopReason::Superseded, true);
                }
                Some(_) => {
                    let _ = reply.send(Err(EngineError::VirtualMicBusy));
                    return;
                }
                None => {}
            }
        }
        let mut profile = self.profile_for(kind, &peer);
        if fallback.is_active() {
            profiles::apply_quality_fallback(&mut profile);
        }
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
            replace,
        }));
        let mut route = new_route(peer, id, kind, true, self.settings.output.volume);
        route.status = RouteStatus::Requesting;
        route.profile = Some(profile);
        route.quality_fallback = fallback;
        route.reply = Some(reply);
        route.deadline = Some(Instant::now() + Duration::from_secs(35));
        self.routes.push(route);
    }

    /// The route that feeds this device's virtual microphone now, if any, ignoring routes with
    /// `except` (the device asking for it). Only one feed is allowed at a time (plan §15.8).
    fn virtual_mic_feed(&self, except: Option<DeviceId>) -> Option<String> {
        self.routes
            .iter()
            .find(|r| {
                r.kind == RouteKind::ReceiveMicToVirtualMic
                    && r.status != RouteStatus::Stopped
                    && Some(r.peer) != except
            })
            .map(Route::key)
    }

    /// Devices this one already feeds from `source` ("system", "apps", "mic"). Paused routes are
    /// not counted: they cost nothing, and a device reconnecting must not find its own place
    /// taken.
    fn receivers_of(&self, source: &str) -> u32 {
        self.routes
            .iter()
            .filter(|r| !matches!(r.status, RouteStatus::Stopped | RouteStatus::Paused))
            .filter(|r| r.kind.local_is_source() && r.kind.endpoints().0 == source)
            .count() as u32
    }

    /// The multi-device picture for the UIs: how many devices listen, the limit, and an estimate
    /// of the bandwidth and CPU it costs (plan §19.2). See [`StreamingLoad`].
    fn streaming_load(&self) -> StreamingLoad {
        let sending: Vec<&Route> = self
            .routes
            .iter()
            .filter(|r| !matches!(r.status, RouteStatus::Stopped | RouteStatus::Paused))
            .filter(|r| r.kind.local_is_source())
            .collect();
        let mut kbps = 0;
        let mut cpu_pct = 0;
        let mut receivers = 0;
        let mut per_receiver_kbps = None;
        for source in ["system", "apps", "mic", "mixed"] {
            let group: Vec<&&Route> = sending
                .iter()
                .filter(|r| r.kind.endpoints().0 == source)
                .collect();
            let Some(first) = group.first().and_then(|r| r.profile.as_ref()) else {
                continue;
            };
            let each = profile_kbps(first);
            kbps += each * group.len() as u32;
            cpu_pct += encoder_cpu_pct(first) + PER_RECEIVER_CPU_PCT * group.len() as u32;
            if group.len() as u32 >= receivers {
                receivers = group.len() as u32;
                per_receiver_kbps = Some(each);
            }
        }
        // Nothing is running yet: estimate from the settings, so the cost of the first extra
        // device can be shown before it is added.
        let per_receiver_kbps = per_receiver_kbps.unwrap_or_else(|| {
            let s = &self.settings.stream;
            profile_kbps(&build_profile(
                s.latency_profile(),
                s.quality(),
                2,
                s.redundancy_always(),
            ))
        });
        let max_receivers = self.settings.max_receivers;
        StreamingLoad {
            receivers,
            max_receivers,
            safe_receivers: (SAFE_TOTAL_KBPS / per_receiver_kbps.max(1)).clamp(1, max_receivers),
            kbps,
            cpu_pct,
            per_receiver_kbps,
            per_receiver_cpu_pct: PER_RECEIVER_CPU_PCT,
        }
    }

    fn check_local_capability(&self, kind: RouteKind) -> Result<(), EngineError> {
        let caps = self.local_capabilities();
        // Plan §19.2: a source feeds at most `max_receivers` devices at once.
        if kind.local_is_source()
            && self.receivers_of(kind.endpoints().0) >= self.settings.max_receivers
        {
            return Err(EngineError::TooManyReceivers);
        }
        match kind {
            RouteKind::SendSystemAudio if !caps.system_audio => {
                Err(EngineError::LoopbackUnsupported)
            }
            RouteKind::SendAppAudio if !caps.app_audio => Err(EngineError::LoopbackUnsupported),
            RouteKind::SendMixed if !caps.mixed => Err(EngineError::LoopbackUnsupported),
            RouteKind::SendMixed if !self.hooks.microphone_permitted() => {
                Err(EngineError::MicPermissionDenied)
            }
            RouteKind::ReceiveMicToVirtualMic if !caps.virtual_mic => {
                Err(EngineError::VirtualMicMissing)
            }
            RouteKind::SendMicToVirtualMic | RouteKind::SendMicToSpeaker
                if !self.hooks.microphone_permitted() =>
            {
                Err(EngineError::MicPermissionDenied)
            }
            _ => Ok(()),
        }
    }

    /// Profile proposed for a route with `peer`: global stream settings plus the device's profile.
    fn profile_for(&self, kind: RouteKind, peer: &DeviceId) -> StreamProfile {
        let mut s = self.settings.stream_for(&peer.to_hex());
        // Anti-flap: a connection that keeps dropping streams with the Stable profile for a
        // while. A custom buffer is the user's explicit choice and stays.
        if self.stabilized.contains_key(peer) && s.latency != LatencyMode::Custom {
            s.latency = LatencyMode::Stable;
        }
        let channels = if kind.is_mic() { 1 } else { 2 };
        // "Auto" is Opus unless the app says the device can afford uncompressed audio right now
        // (plan §14.6: a charger and a link with room for it). A quality the user picked stands.
        let quality = match s.quality() {
            Quality::Auto if self.prefer_lossless => Quality::Lossless,
            q => q,
        };
        let mut p = build_profile(
            s.latency_profile(),
            quality,
            channels,
            s.redundancy_always(),
        );
        if kind.is_mic() && p.frame_us < 10_000 {
            // Noise suppression works on 10 ms frames.
            p.frame_us = 10_000;
        }
        p.denoise = self.denoise_at_peer(kind, peer);
        p
    }

    /// Whether the peer should suppress noise in a microphone stream this device sends it
    /// ("Noise suppression → on the other device", plan §4.3/§15.7). False towards peers that do
    /// not understand it, so this device denoises instead and the setting still does something.
    fn denoise_at_peer(&self, kind: RouteKind, peer: &DeviceId) -> bool {
        if !kind.is_mic() || !kind.local_is_source() {
            return false;
        }
        let peer_can = self.sessions.get(peer).is_some_and(|s| {
            Capabilities(s.hello.capabilities).has(Capabilities::FEATURE_RECEIVER_DENOISE)
        });
        self.settings.mic.denoise_placement(peer_can).1
    }

    fn on_route_request(&mut self, peer: DeviceId, req: RouteRequest) {
        let local_is_source = !req.requester_is_source;
        let Some(kind) =
            RouteKind::from_endpoints(&req.source_endpoint, &req.sink_endpoint, local_is_source)
        else {
            self.reject(peer, req.route, StopReason::UnsupportedEndpoint);
            return;
        };
        if let Err(e) = self.check_local_capability(kind) {
            let reason = match e {
                EngineError::MicPermissionDenied => StopReason::PermissionDenied,
                EngineError::TooManyReceivers => StopReason::DeviceBusy,
                _ => StopReason::UnsupportedEndpoint,
            };
            if e == EngineError::TooManyReceivers {
                self.audit_peer(AuditKind::RouteDenied, &peer, Some(kind), "limit");
                let name = self.peer_name(&peer);
                self.error_notice(&e, vec![name]);
            }
            self.reject(peer, req.route, reason);
            return;
        }
        // A second device asking for the virtual microphone is refused until its user confirms
        // "Replace current microphone source?" and asks again with `replace` (plan §8.2).
        if kind == RouteKind::ReceiveMicToVirtualMic
            && let Some(current) = self.virtual_mic_feed(Some(peer))
        {
            if !req.replace {
                self.reject(peer, req.route, StopReason::VirtualMicBusy);
                return;
            }
            self.stop_route_by_key(&current, StopReason::Superseded, true);
        }
        let permission = if local_is_source {
            if kind.is_mic() {
                PermissionKind::UseMyMicrophone
            } else {
                PermissionKind::ReceiveMyAudio
            }
        } else {
            PermissionKind::SendAudioToMe
        };
        let policy = self
            .trust
            .get(&peer)
            .map(|d| d.permissions.policy(permission))
            .unwrap_or(Policy::Deny);
        let resumed = self.resume.resumed_recently(&peer, Instant::now())
            && self.routes.iter().any(|r| {
                r.peer == peer
                    && r.kind == kind
                    && r.status == RouteStatus::Paused
                    && !r.requested_locally
            });
        match policy {
            Policy::Allow => self.accept_route(peer, req, kind),
            Policy::Deny => {
                self.audit_peer(AuditKind::RouteDenied, &peer, Some(kind), "permission");
                self.reject(peer, req.route, StopReason::PermissionDenied);
            }
            // The user approved this route before the connection dropped, and the peer proved with
            // a resume token that this connection continues that session: no second prompt.
            Policy::Ask if resumed => self.accept_route(peer, req, kind),
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
                if pending.kind.is_mic() {
                    PermissionKind::UseMyMicrophone
                } else {
                    PermissionKind::ReceiveMyAudio
                }
            } else {
                PermissionKind::SendAudioToMe
            };
            let policy = if accept { Policy::Allow } else { Policy::Deny };
            let _ = self
                .trust
                .update(&pending.peer, |d| d.permissions.set(kind, policy));
            self.audit_permission(&pending.peer, kind, policy);
        }
        let decision = if accept {
            AuditKind::RouteApproved
        } else {
            AuditKind::RouteDenied
        };
        self.audit_peer(decision, &pending.peer, Some(pending.kind), "user");
        if accept {
            self.accept_route(pending.peer, pending.request, pending.kind);
        } else {
            self.reject(
                pending.peer,
                pending.request.route,
                StopReason::PermissionDenied,
            );
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
        let mut profile = req
            .profile
            .clone()
            .unwrap_or_else(|| self.profile_for(kind, &peer));
        sanitize_profile(&mut profile);
        if kind.local_is_source() {
            // Where this device's microphone is denoised is its own setting, not the requester's.
            profile.denoise = self.denoise_at_peer(kind, &peer);
        } else {
            // The receiving side's latency preference wins.
            let local = self.profile_for(kind, &peer);
            profile.jitter_min_ms = local.jitter_min_ms;
            profile.jitter_max_ms = local.jitter_max_ms;
        }
        let id = req.route as u8;
        // A paused copy from before a reconnect is replaced. Route ids restart with each session,
        // so a paused route that happens to reuse this id goes too (its requester asks again).
        self.routes.retain(|r| {
            !(r.peer == peer && r.status == RouteStatus::Paused && (r.kind == kind || r.id == id))
        });
        if self.routes.iter().any(|r| r.peer == peer && r.id == id) {
            return; // duplicate request
        }
        let mut route = new_route(peer, id, kind, false, self.settings.output.volume);
        route.profile = Some(profile);
        route.deadline = Some(Instant::now() + PIPELINE_START_DEADLINE);
        // RouteAccept is sent once the audio device is open (activate_route).
        match self.begin_pipelines(&mut route) {
            Ok(ready) => {
                self.routes.push(route);
                if ready {
                    self.activate_route(peer, id);
                }
            }
            Err(e) => {
                let reason = match e {
                    EngineError::MicPermissionDenied => StopReason::PermissionDenied,
                    EngineError::VirtualMicMissing | EngineError::LoopbackUnsupported => {
                        StopReason::UnsupportedEndpoint
                    }
                    _ => StopReason::AudioDeviceLost,
                };
                self.reject(peer, req.route, reason);
                let name = self.peer_name(&peer);
                self.error_notice(&e, vec![name]);
            }
        }
    }

    fn on_route_accept(&mut self, peer: DeviceId, acc: RouteAccept) {
        let key = route_key(&peer, acc.route as u8);
        let Some(pos) = self
            .routes
            .iter()
            .position(|r| r.key() == key && r.status == RouteStatus::Requesting)
        else {
            return;
        };
        let mut route = self.routes.remove(pos);
        if let Some(mut p) = acc.profile {
            sanitize_profile(&mut p);
            route.profile = Some(p);
        }
        route.status = RouteStatus::Starting;
        route.deadline = Some(Instant::now() + PIPELINE_START_DEADLINE);
        match self.begin_pipelines(&mut route) {
            Ok(ready) => {
                let id = route.id;
                self.routes.push(route);
                if ready {
                    self.activate_route(peer, id);
                }
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

    fn stop_route_by_key(&mut self, key: &str, reason: StopReason, notify_peer: bool) {
        let Some(pos) = self.routes.iter().position(|r| r.key() == key) else {
            return;
        };
        let mut route = self.routes.remove(pos);
        self.release_pipelines(&mut route);
        if matches!(route.status, RouteStatus::Active | RouteStatus::Paused) {
            info!(
                route = key,
                code = stop_reason_code(reason),
                reason = ?reason,
                "route stopped"
            );
            self.audit_peer(
                AuditKind::RouteStopped,
                &route.peer,
                Some(route.kind),
                &format!("{reason:?}"),
            );
        }
        if notify_peer {
            if let Some(s) = self.sessions.get(&route.peer) {
                s.send(Body::RouteStop(RouteStop {
                    route: route.id as u32,
                    reason: reason as i32,
                }));
            }
        }
        if let Some(reply) = route.reply.take() {
            let _ = reply.send(Err(EngineError::RouteNotFound));
        }
        if route.kind == RouteKind::SendSystemAudio
            && self.settings.capture.mute_local_speakers
            && self.speakers_muted_by.is_empty()
        {
            self.hooks.set_speakers_muted(false);
        }
        // Saved routes: "Keep running" entries go only when the user stops the route; entries
        // saved by "Resume streams after restart" go when the route itself ends. Session-level
        // removal (`Unspecified`) and restarts (`Superseded`) keep both.
        if !matches!(reason, StopReason::Superseded | StopReason::Unspecified) {
            let peer_id = route.peer.to_hex();
            let before = self.settings.saved_routes.len();
            self.settings.saved_routes.retain(|s| {
                !(s.matches(&peer_id, route.kind) && (!s.keep || reason == StopReason::UserStopped))
            });
            if self.settings.saved_routes.len() != before {
                self.save_settings();
            }
        }
        self.update_keep_alive();
    }

    fn pause_routes_for(&mut self, peer: DeviceId) {
        for i in 0..self.routes.len() {
            if self.routes[i].peer != peer {
                continue;
            }
            self.release_pipelines_at(i);
            let r = &mut self.routes[i];
            match r.status {
                RouteStatus::Active => {
                    r.status = RouteStatus::Paused;
                    r.paused_at = Some(Instant::now());
                }
                RouteStatus::Paused => {}
                _ => {
                    if let Some(reply) = r.reply.take() {
                        let _ = reply.send(Err(EngineError::Unreachable));
                    }
                    r.status = RouteStatus::Stopped;
                }
            }
        }
        self.routes.retain(|r| r.status != RouteStatus::Stopped);
        self.update_keep_alive();
    }

    fn remove_routes_for(&mut self, peer: DeviceId) {
        let keys: Vec<String> = self
            .routes
            .iter()
            .filter(|r| r.peer == peer)
            .map(Route::key)
            .collect();
        for k in keys {
            self.stop_route_by_key(&k, StopReason::Unspecified, false);
        }
    }

    /// Re-request routes after a reconnect (only the side that originally requested does this),
    /// and routes saved to run again after a restart.
    fn resume_routes(&mut self, peer: DeviceId) {
        let mut kinds: Vec<RouteKind> = self
            .routes
            .iter()
            .filter(|r| r.peer == peer && r.status == RouteStatus::Paused && r.requested_locally)
            .map(|r| r.kind)
            .collect();
        self.routes.retain(|r| {
            !(r.peer == peer && r.status == RouteStatus::Paused && r.requested_locally)
        });
        let peer_hex = peer.to_hex();
        let resume_all = self.settings.resume_routes_on_start;
        for saved in &self.settings.saved_routes {
            if saved.peer_id == peer_hex
                && (saved.keep || resume_all)
                && !kinds.contains(&saved.kind)
            {
                kinds.push(saved.kind);
            }
        }
        for kind in kinds {
            let (tx, _rx) = oneshot::channel();
            // Restoring a saved route never takes the virtual microphone from a live one.
            self.start_route(peer, kind, false, tx);
            if let Some(r) = self
                .routes
                .iter_mut()
                .rev()
                .find(|r| r.peer == peer && r.kind == kind)
            {
                r.keep_running = self
                    .settings
                    .saved_routes
                    .iter()
                    .any(|s| s.matches(&peer_hex, kind) && s.keep);
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
                    if r.kind.is_mic() {
                        PermissionKind::UseMyMicrophone
                    } else {
                        PermissionKind::ReceiveMyAudio
                    }
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
        self.flaps.remove(&id);
        self.stabilized.remove(&id);
        self.remove_routes_for(id);
        self.cancel_dial(&id);
        self.dials.remove(&id);
        self.resume.forget(&id);
        self.end_network_test(id, EngineError::Revoked);
        if let Some(s) = self.sessions.remove(&id) {
            self.conn_index.remove(&s.conn_id);
            let _ = s.tx.send(SessionCmd::Close(StopReason::PermissionRevoked));
        }
        self.forget_speaker_mutes(id);
    }

    /// The session with `id` ended: its "Mute PC" in either direction ends with it, so a phone
    /// that disconnects never leaves this computer's speakers muted.
    fn forget_speaker_mutes(&mut self, id: DeviceId) {
        self.peer_speakers_muted.remove(&id);
        if self.speakers_muted_by.remove(&id) && self.speakers_muted_by.is_empty() {
            let sending_muted = self.settings.capture.mute_local_speakers
                && self
                    .routes
                    .iter()
                    .any(|r| r.kind == RouteKind::SendSystemAudio);
            if !sending_muted {
                self.hooks.set_speakers_muted(false);
            }
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
            if m.muted {
                self.speakers_muted_by.insert(peer);
            } else {
                self.speakers_muted_by.remove(&peer);
            }
            if !self.hooks.set_speakers_muted(m.muted) {
                let name = self.peer_name(&peer);
                self.notice(
                    "notice.muteSpeakersUnsupported",
                    vec![name],
                    Severity::Info,
                    None,
                );
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
        // "Resilient: Auto" may switch redundancy on by itself; "Always" and "Off" may not.
        let auto = self.settings.stream_for(&peer.to_hex()).redundancy_auto();
        let Some(r) = self.routes.iter_mut().find(|r| r.key() == key) else {
            return;
        };
        let total = stats.packets_received.saturating_add(stats.packets_lost);
        r.loss_pct = if total > 0 {
            stats.packets_lost as f64 * 100.0 / total as f64
        } else {
            0.0
        };
        if let (Some(c), Some(p)) = (&r.sender_controls, &r.profile) {
            if p.adaptive_bitrate && p.codec == Codec::Opus as u32 {
                let current = c.bitrate.load(Ordering::Relaxed);
                if r.loss_pct > 2.0 {
                    c.bitrate
                        .store((current * 3 / 4).max(32_000), Ordering::Relaxed);
                    c.expected_loss_pct
                        .store(r.loss_pct.round() as u32, Ordering::Relaxed);
                    r.stable_secs = 0;
                } else if r.loss_pct < 0.5 {
                    r.stable_secs += 1;
                    if r.stable_secs >= 10 {
                        c.bitrate.store(
                            (current * 5 / 4).min(p.bitrate.max(128_000)),
                            Ordering::Relaxed,
                        );
                        c.expected_loss_pct.store(0, Ordering::Relaxed);
                        r.stable_secs = 0;
                    }
                }
            }
        }
        // Automatic redundancy (plan §15.6): on above 1 % loss, off after 10 clean seconds.
        // "Always" keeps it on through the profile instead, and "Off" never turns it on.
        if let (true, Some(c), Some(p)) = (auto, &r.sender_controls, &r.profile) {
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
        self.tick_network_tests(now);
        self.resume.prune(now);
        self.prune_encoders();
        if now.duration_since(self.local_addrs_at) >= LOCAL_ADDRESS_REFRESH {
            self.refresh_local_addresses();
        }
        if now.duration_since(self.trust_flushed_at) >= TRUST_FLUSH_INTERVAL {
            self.flush_trust();
        }

        // Route deadlines, paused route expiry, stats reports.
        let expired: Vec<String> = self
            .routes
            .iter()
            .filter(|r| {
                r.deadline.is_some_and(|d| now > d)
                    || (r.status == RouteStatus::Paused
                        && !r.keep_running
                        && r.paused_at
                            .is_some_and(|p| now.duration_since(p) > Duration::from_secs(120)))
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
                let (dr, dm) = (
                    received.saturating_sub(r.last_received),
                    missing.saturating_sub(r.last_missing),
                );
                r.last_received = received;
                r.last_missing = missing;
                r.loss_pct = if dr + dm > 0 {
                    dm as f64 * 100.0 / (dr + dm) as f64
                } else {
                    0.0
                };
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

        self.update_link_health();
        self.update_quality_fallback();
        self.expire_stable_holds(now);
        self.refused_audited
            .retain(|_, t| now.duration_since(*t) < REFUSED_AUDIT_INTERVAL);
        if self.settings.expire_debug_logging(now_unix()) {
            crate::logging::set_debug(false);
            self.save_settings();
        }

        // Capabilities can change while connected (e.g. a virtual microphone was installed).
        // Re-send Hello so peers enable or disable the matching tasks without reconnecting.
        let caps = self.capability_bits().0;
        if caps != self.announced_caps {
            self.announced_caps = caps;
            let hello = self.local_hello(None);
            for s in self.sessions.values() {
                s.send(Body::Hello(hello.clone()));
            }
            self.update_discovery();
        }

        self.check_virtual_mic_use();
        self.check_capture_health();
        self.check_mic_feedback(now);
        self.check_audio_idle(now);

        // Pending prompts expire.
        let expired_requests: Vec<u64> = self
            .requests
            .iter()
            .filter(|(_, r)| now > r.expires)
            .map(|(k, _)| *k)
            .collect();
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
                .filter(|d| {
                    d.auto_connect && !d.blocked && !self.sessions.contains_key(&d.device_id)
                })
                .map(|d| d.device_id)
                .collect();
            for id in candidates {
                self.try_dial_trusted(id);
            }
        }
    }

    /// Let noise suppression switch itself off while the machine cannot keep up (plan §8.3).
    fn check_capture_health(&mut self) {
        let overruns: u64 = self
            .encoders
            .iter()
            .filter(|(k, _)| k.is_mic())
            .filter_map(|(_, slot)| match slot {
                pipelines::EncoderSlot::Running(s) => {
                    Some(s.controls.capture_overruns.load(Ordering::Relaxed))
                }
                pipelines::EncoderSlot::Starting { .. } => None,
            })
            .sum();
        let change = self
            .denoise
            .step(overruns, self.settings.mic.noise_suppression);
        let Some(change) = change else { return };
        self.update_mic_groups();
        match change {
            DenoiseChange::Suspended => {
                warn!("capture is missing deadlines; noise suppression switched itself off");
                self.notice("notice.denoiseSuspended", vec![], Severity::Warning, None);
            }
            DenoiseChange::Resumed => info!("capture is keeping up; noise suppression back on"),
        }
    }

    /// Warn once when monitoring the microphone on this device's speakers starts to howl
    /// (plan §8.2). The detector only runs while the monitor does.
    fn check_mic_feedback(&mut self, now: Instant) {
        if self.monitor.as_ref().is_some_and(MicMonitor::take_feedback) {
            let fresh = self
                .mic_feedback_at
                .is_none_or(|t| now.duration_since(t) > FEEDBACK_NOTICE_INTERVAL);
            self.mic_feedback_at = Some(now);
            if fresh {
                warn!("microphone feedback loop detected while monitoring");
                self.notice("notice.micFeedback", vec![], Severity::Warning, None);
            }
        }
        // The banner goes away once the loop has been quiet for a while, or the monitor stopped.
        if self
            .mic_feedback_at
            .is_some_and(|t| now.duration_since(t) > FEEDBACK_BANNER)
            || self.monitor.is_none()
        {
            self.mic_feedback_at = None;
        }
    }

    fn update_keep_alive(&mut self) {
        let mut ka = KeepAlive::default();
        for r in &self.routes {
            if r.status != RouteStatus::Active {
                continue;
            }
            match r.kind {
                // A mixed stream records the microphone too.
                RouteKind::SendMicToSpeaker
                | RouteKind::SendMicToVirtualMic
                | RouteKind::SendMixed => ka.microphone = true,
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
        // Sending this computer's audio, or a peer's "Mute PC", may have muted its speakers:
        // never leave them that way.
        if !self.speakers_muted_by.is_empty()
            || self.settings.capture.mute_local_speakers
                && self
                    .routes
                    .iter()
                    .any(|r| r.kind == RouteKind::SendSystemAudio)
        {
            self.hooks.set_speakers_muted(false);
        }
        for (_, s) in self.sessions.drain() {
            let _ = s.tx.send(SessionCmd::Close(StopReason::UserStopped));
        }
        self.abort_network_tests();
        // Release audio here, before completion is signalled (dropping joins the audio threads).
        for r in &mut self.routes {
            r.subscription = None;
            r.receiver = None;
        }
        self.routes.clear();
        self.encoders.clear();
        self.monitor = None;
        self.flush_trust();
        self.endpoint.close();
        if let Some(tcp) = &self.tcp {
            tcp.close();
        }
    }

    // ============================================================ state snapshot

    fn publish(&mut self) {
        self.revision += 1;
        self.last_publish = Instant::now();
        let now = Instant::now();
        let local_id = self.identity.device_id();

        let mut peers: Vec<PeerView> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for d in self.trust.list() {
            seen.insert(d.device_id);
            peers.push(self.peer_view(d.device_id, Some(d)));
        }
        for id in self
            .discovered
            .keys()
            .chain(self.sessions.keys())
            .copied()
            .collect::<Vec<_>>()
        {
            if seen.insert(id) {
                peers.push(self.peer_view(id, None));
            }
        }
        peers.sort_by(|a, b| {
            b.trusted
                .cmp(&a.trusted)
                .then(b.online.cmp(&a.online))
                .then(a.name.cmp(&b.name))
        });

        let routes = self
            .routes
            .iter()
            .map(|r| {
                let rtt = self
                    .sessions
                    .get(&r.peer)
                    .map(|s| s.conn.stats().rtt.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0);
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
                let frame_ms = r
                    .profile
                    .as_ref()
                    .map(|p| p.frame_us as f64 / 1000.0)
                    .unwrap_or(10.0);
                if let Some(c) = &r.receiver_controls {
                    stats.buffer_ms = c.buffer_ms.get() as f64;
                    stats.jitter_ms = c.jitter_ms.get() as f64;
                    stats.underruns = c.underruns.load(Ordering::Relaxed);
                    stats.drift_ppm = c.drift_ppm.load(Ordering::Relaxed);
                    stats.level_db = c.level_db.get();
                    stats.encode_ms = frame_ms;
                    stats.network_ms = rtt / 2.0;
                    stats.output_ms = c.device_latency_ms.load(Ordering::Relaxed) as f64;
                    stats.latency_ms =
                        stats.buffer_ms + stats.encode_ms + stats.output_ms + stats.network_ms;
                }
                if let Some(c) = &r.sender_controls {
                    stats.level_db = c.level_db.get();
                    stats.clipping = c.clipping.load(Ordering::Relaxed);
                    if let Some(remote) = &r.remote_stats {
                        stats.buffer_ms = remote.buffer_ms as f64;
                        stats.jitter_ms = remote.jitter_us as f64 / 1000.0;
                        stats.underruns = remote.underruns as u64;
                        stats.drift_ppm = remote.drift_ppm;
                        stats.capture_ms = frame_ms;
                        stats.encode_ms = frame_ms;
                        stats.network_ms = rtt / 2.0;
                        stats.output_ms = remote.output_latency_ms as f64;
                        stats.latency_ms = stats.buffer_ms
                            + stats.capture_ms
                            + stats.encode_ms
                            + stats.output_ms
                            + stats.network_ms;
                    }
                }
                RouteView {
                    route_id: r.key(),
                    peer_id: r.peer.to_hex(),
                    peer_name: self.peer_name(&r.peer),
                    kind: r.kind,
                    status: r.status,
                    started_unix: r.started_unix,
                    elapsed_secs: if r.status == RouteStatus::Active {
                        now.duration_since(r.started).as_secs()
                    } else {
                        0
                    },
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

        let mic_controls = || {
            self.routes
                .iter()
                .filter(|r| r.kind.is_mic())
                .filter_map(|r| r.sender_controls.as_ref())
        };
        let mic_level_db = mic_controls()
            .map(|c| c.level_db.get())
            .fold(-120.0f32, f32::max);
        let mic_clipping = mic_controls().any(|c| c.clipping.load(Ordering::Relaxed));

        let state = EngineState {
            revision: self.revision,
            local: LocalDevice {
                device_id: local_id.to_hex(),
                display_code: local_id.display_code(),
                name: self.settings.device_name.clone(),
                platform: self.hooks.platform().to_string(),
                port: self.endpoint.local_port(),
                tcp_port: self.tcp.as_ref().map_or(0, |t| t.local_port()),
                addresses: self.local_addrs.clone(),
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
            mic_clipping,
            mic_feedback: self.mic_feedback_at.is_some(),
            noise_suppression_suspended: self.denoise.suspended(),
            network_tests: self.network_test_views(),
            mic_muted: self.mic_muted,
            streaming: self.streaming_load(),
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
        let connection = if let Some(s) = session {
            if s.health.is_degraded() {
                ConnectionStatus::Degraded
            } else {
                ConnectionStatus::Connected
            }
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
                let loss = if st.sent_packets > 0 {
                    st.lost_packets as f64 * 100.0 / st.sent_packets as f64
                } else {
                    0.0
                };
                let jitter = self
                    .routes
                    .iter()
                    .filter(|r| r.peer == id)
                    .filter_map(|r| {
                        r.receiver_controls
                            .as_ref()
                            .map(|c| c.jitter_ms.get() as f64)
                    })
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
            addresses: advert
                .map(|a| a.addresses.iter().map(ToString::to_string).collect())
                .unwrap_or_default(),
            permissions: trusted.map(|t| t.permissions),
            auto_connect: trusted.is_some_and(|t| t.auto_connect),
            blocked: trusted.is_some_and(|t| t.blocked),
            last_seen_unix: trusted.map(|t| t.last_seen_unix).unwrap_or(0),
            can_send_system_audio: caps.has(Capabilities::SOURCE_SYSTEM_AUDIO),
            can_send_app_audio: caps.has(Capabilities::SOURCE_APP_AUDIO),
            can_send_mic: caps.has(Capabilities::SOURCE_MICROPHONE),
            can_send_mixed: caps.has(Capabilities::SOURCE_MIXED),
            can_play: caps.has(Capabilities::SINK_SPEAKER),
            has_virtual_mic: caps.has(Capabilities::SINK_VIRTUAL_MIC),
            transport: match session.map(|s| s.conn.transport()) {
                Some(TransportKind::Quic) => "quic",
                Some(TransportKind::Tcp) => "tcp",
                None => "",
            }
            .to_string(),
            remote_address: session
                .map(|s| s.conn.remote_address().to_string())
                .unwrap_or_default(),
            speakers_muted: session.is_some()
                && self.peer_speakers_muted.get(&id).copied().unwrap_or(false),
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

/// The first of two connection attempts to succeed; the other is dropped (cancelled). When both
/// fail, the first attempt's error is reported (network errors say more than a refused USB port).
async fn first_success<A, B>(a: A, b: B) -> Result<SecureConnection, EngineError>
where
    A: Future<Output = Result<SecureConnection, EngineError>>,
    B: Future<Output = Result<SecureConnection, EngineError>>,
{
    tokio::pin!(a, b);
    let mut a_error: Option<EngineError> = None;
    let mut b_failed = false;
    loop {
        tokio::select! {
            result = &mut a, if a_error.is_none() => match result {
                Ok(conn) => return Ok(conn),
                Err(e) if b_failed => return Err(e),
                Err(e) => a_error = Some(e),
            },
            result = &mut b, if !b_failed => match result {
                Ok(conn) => return Ok(conn),
                Err(e) => match a_error.take() {
                    Some(first) => return Err(first),
                    None => {
                        b_failed = true;
                        let _ = e;
                    }
                },
            },
        }
    }
}

fn new_route(
    peer: DeviceId,
    id: u8,
    kind: RouteKind,
    requested_locally: bool,
    volume: f32,
) -> Route {
    Route {
        peer,
        id,
        kind,
        status: RouteStatus::Starting,
        requested_locally,
        profile: None,
        started: Instant::now(),
        started_unix: now_unix(),
        encoder: None,
        subscription: None,
        receiver: None,
        start_id: 0,
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
        quality_fallback: QualityFallback::default(),
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
