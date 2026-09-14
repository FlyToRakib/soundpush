//! SoundPush engine — the only API used by the desktop and mobile apps.
//!
//! The engine runs on its own Tokio runtime as a single actor that owns all
//! mutable state. Apps send commands through [`EngineHandle`] and render the
//! immutable [`EngineState`] snapshots it publishes.
#![forbid(unsafe_code)]

mod actor;
pub mod crash;
pub mod error;
mod net;
pub mod nettest;
pub mod pipeline;
pub mod platform;
pub mod reconnect;
mod resume;
mod session;
pub mod settings;
pub mod state;

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, watch};

pub use error::{EngineError, ErrorView, FixAction, Severity};
pub use nettest::{NetworkReport, NetworkTestStatus, NetworkTestView, Recommendation};
pub use platform::{KeepAlive, PlatformHooks};
pub use settings::{DeviceProfile, Settings};
pub use sp_audio_io;
pub use sp_security::{PermissionKind, Permissions, Policy};
pub use state::{EngineState, RouteKind};

use actor::Command;

/// Engine start-up options.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub app_version: String,
    /// Preferred UDP port (0 = ephemeral).
    pub port: u16,
    /// Run mDNS and beacon discovery.
    pub discovery: bool,
    /// Include loopback addresses in pairing codes (tests only).
    pub include_loopback: bool,
    /// Accept TLS-over-TCP connections on loopback, where `adb reverse` delivers a phone's USB
    /// connection. Desktop builds; phones only dial.
    pub tcp_listener: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            port: sp_transport::DEFAULT_PORT,
            discovery: true,
            include_loopback: false,
            tcp_listener: true,
        }
    }
}

struct Inner {
    runtime: Option<tokio::runtime::Runtime>,
    commands: mpsc::UnboundedSender<Command>,
    state: watch::Receiver<Arc<EngineState>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown { done: None });
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

/// Cheap to clone; the engine stops when the last handle is dropped.
#[derive(Clone)]
pub struct EngineHandle {
    inner: Arc<Inner>,
}

impl EngineHandle {
    /// Start the engine. Safe to call from inside or outside another async runtime.
    pub fn start(hooks: Arc<dyn PlatformHooks>, config: EngineConfig) -> Result<Self, EngineError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("sp-engine")
            .enable_all()
            .build()
            .map_err(|e| EngineError::Internal(e.to_string()))?;

        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        runtime.spawn(async move {
            let _ = ready_tx.send(actor::spawn(hooks, config).await);
        });
        let (commands, state) = ready_rx
            .recv()
            .map_err(|_| EngineError::Internal("engine failed to start".into()))??;

        Ok(Self {
            inner: Arc::new(Inner {
                runtime: Some(runtime),
                commands,
                state,
            }),
        })
    }

    /// Stop streaming, tell peers and release what the engine holds (muted speakers, wake locks).
    /// Waits up to `timeout`. For the moment before the process exits: dropping the handle
    /// only queues the shutdown and would not wait for any of it.
    pub fn shutdown(&self, timeout: std::time::Duration) {
        let (done, finished) = std::sync::mpsc::channel();
        if self.send(Command::Shutdown { done: Some(done) }).is_ok() {
            let _ = finished.recv_timeout(timeout);
            // Give the endpoint a moment to put its close frames on the wire.
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }

    /// Latest state snapshot.
    pub fn state(&self) -> Arc<EngineState> {
        self.inner.state.borrow().clone()
    }

    /// Receiver that changes whenever the state changes.
    pub fn subscribe(&self) -> watch::Receiver<Arc<EngineState>> {
        self.inner.state.clone()
    }

    fn send(&self, cmd: Command) -> Result<(), EngineError> {
        self.inner
            .commands
            .send(cmd)
            .map_err(|_| EngineError::Stopped)
    }

    async fn request<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, EngineError>>) -> Command,
    ) -> Result<T, EngineError> {
        let (tx, rx) = oneshot::channel();
        self.send(make(tx))?;
        rx.await.map_err(|_| EngineError::Stopped)?
    }

    // ---------------------------------------------------------------- pairing

    /// Open pairing mode and return the QR code URI to display.
    pub async fn start_pairing(&self) -> Result<String, EngineError> {
        self.request(Command::StartPairing).await
    }

    pub fn stop_pairing(&self) -> Result<(), EngineError> {
        self.send(Command::StopPairing)
    }

    /// Pair by scanning another device's QR code.
    pub async fn pair_with_qr(&self, uri: String) -> Result<(), EngineError> {
        self.request(|reply| Command::PairWithQr { uri, reply })
            .await
    }

    /// Start code pairing with a discovered device (the other device must have pairing open).
    pub async fn pair_with_device(&self, device_id: String) -> Result<(), EngineError> {
        self.request(|reply| Command::PairWithDevice { device_id, reply })
            .await
    }

    /// Start code pairing with a manually entered address (IP, IP:port or hostname).
    pub async fn pair_with_address(&self, address: String) -> Result<(), EngineError> {
        self.request(|reply| Command::PairWithAddress { address, reply })
            .await
    }

    /// Accept or reject a code comparison prompt.
    pub fn confirm_pairing(&self, device_id: String, accept: bool) -> Result<(), EngineError> {
        self.send(Command::ConfirmPairing { device_id, accept })
    }

    // ---------------------------------------------------------------- devices

    pub fn connect(&self, device_id: String) -> Result<(), EngineError> {
        self.send(Command::Connect { device_id })
    }

    pub fn disconnect(&self, device_id: String) -> Result<(), EngineError> {
        self.send(Command::Disconnect { device_id })
    }

    pub fn forget_device(&self, device_id: String) -> Result<(), EngineError> {
        self.send(Command::ForgetDevice { device_id })
    }

    pub fn set_device_blocked(&self, device_id: String, blocked: bool) -> Result<(), EngineError> {
        self.send(Command::SetBlocked { device_id, blocked })
    }

    pub fn rename_device(
        &self,
        device_id: String,
        alias: Option<String>,
    ) -> Result<(), EngineError> {
        self.send(Command::RenameDevice { device_id, alias })
    }

    pub fn set_auto_connect(&self, device_id: String, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetAutoConnect { device_id, enabled })
    }

    pub fn set_permission(
        &self,
        device_id: String,
        kind: PermissionKind,
        policy: Policy,
    ) -> Result<(), EngineError> {
        self.send(Command::SetPermission {
            device_id,
            kind,
            policy,
        })
    }

    /// Set a device's stream profile (`None` clears it). Running routes pick it up immediately.
    pub fn set_device_profile(
        &self,
        device_id: String,
        profile: Option<DeviceProfile>,
    ) -> Result<(), EngineError> {
        self.send(Command::SetDeviceProfile { device_id, profile })
    }

    // ---------------------------------------------------------------- diagnostics

    /// Measure RTT, jitter, loss and achievable bitrate to a connected device over the media path
    /// (about ten seconds). Progress and the last result also appear in `EngineState::network_tests`.
    pub async fn run_network_test(&self, device_id: String) -> Result<NetworkReport, EngineError> {
        self.request(|reply| Command::RunNetworkTest { device_id, reply })
            .await
    }

    pub fn cancel_network_test(&self, device_id: String) -> Result<(), EngineError> {
        self.send(Command::CancelNetworkTest { device_id })
    }

    // ---------------------------------------------------------------- routes

    /// Start a route with a connected device. Returns the route id.
    pub async fn start_route(
        &self,
        device_id: String,
        kind: RouteKind,
    ) -> Result<String, EngineError> {
        self.request(|reply| Command::StartRoute {
            device_id,
            kind,
            reply,
        })
        .await
    }

    pub fn stop_route(&self, route_id: String) -> Result<(), EngineError> {
        self.send(Command::StopRoute { route_id })
    }

    pub fn set_route_volume(&self, route_id: String, volume: f32) -> Result<(), EngineError> {
        self.send(Command::SetRouteVolume { route_id, volume })
    }

    pub fn set_route_muted(&self, route_id: String, muted: bool) -> Result<(), EngineError> {
        self.send(Command::SetRouteMuted { route_id, muted })
    }

    pub fn set_route_keep_running(&self, route_id: String, keep: bool) -> Result<(), EngineError> {
        self.send(Command::SetRouteKeepRunning { route_id, keep })
    }

    /// Mute the physical speakers of a connected device ("Mute PC").
    pub fn set_peer_speakers_muted(
        &self,
        device_id: String,
        muted: bool,
    ) -> Result<(), EngineError> {
        self.send(Command::SetPeerSpeakersMuted { device_id, muted })
    }

    /// Answer an incoming route request. `remember` stores the decision as the device's permission.
    pub fn respond_route_request(
        &self,
        request_id: u64,
        accept: bool,
        remember: bool,
    ) -> Result<(), EngineError> {
        self.send(Command::RespondRouteRequest {
            request_id,
            accept,
            remember,
        })
    }

    // ---------------------------------------------------------------- local audio

    pub fn set_mic_muted(&self, muted: bool) -> Result<(), EngineError> {
        self.send(Command::SetMicMuted { muted })
    }

    pub fn set_mic_monitor(&self, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetMicMonitor { enabled })
    }

    pub fn refresh_audio_devices(&self) -> Result<(), EngineError> {
        self.send(Command::RefreshAudioDevices)
    }

    /// The OS reported an audio device change. The device list is refreshed, and running routes
    /// that use the system default device of a changed direction reopen on the new default.
    pub fn audio_devices_changed(
        &self,
        default_input_changed: bool,
        default_output_changed: bool,
    ) -> Result<(), EngineError> {
        self.send(Command::AudioDevicesChanged {
            default_input: default_input_changed,
            default_output: default_output_changed,
        })
    }

    // ---------------------------------------------------------------- settings & misc

    pub async fn update_settings(&self, settings: Settings) -> Result<Settings, EngineError> {
        self.request(|reply| Command::UpdateSettings {
            settings: Box::new(settings),
            reply,
        })
        .await
    }

    pub fn dismiss_notice(&self, id: u64) -> Result<(), EngineError> {
        self.send(Command::DismissNotice { id })
    }

    /// Tell the engine the OS reported a network change (retry connections now).
    pub fn network_changed(&self) -> Result<(), EngineError> {
        self.send(Command::NetworkChanged)
    }

    /// Drop the connection to a device without a goodbye, as a network failure would. The engine
    /// then reconnects and resumes as usual. For tests and troubleshooting.
    #[doc(hidden)]
    pub fn simulate_connection_loss(&self, device_id: String) -> Result<(), EngineError> {
        self.send(Command::SimulateConnectionLoss { device_id })
    }

    /// Apps call this when moving between foreground and background (battery policy).
    pub fn set_foreground(&self, foreground: bool) -> Result<(), EngineError> {
        self.send(Command::SetForeground { foreground })
    }
}
