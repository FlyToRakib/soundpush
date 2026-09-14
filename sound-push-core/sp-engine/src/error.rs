//! Engine error taxonomy shared by every platform.
//!
//! Each error maps to a stable message key (localized by the UI), a severity,
//! whether retrying can help, and an optional one-tap fix action.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A fix the UI can offer as the primary button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FixAction {
    OpenMicPermissionSettings,
    InstallVirtualMic,
    OpenFirewallFix,
    SwitchToUsb,
    UpdatePeerApp,
    RetryConnection,
    PairAgain,
    OpenBatterySettings,
    ChooseAnotherDevice,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("device not found")]
    DeviceNotFound,
    #[error("device unreachable")]
    Unreachable,
    #[error("network blocks local devices")]
    NetworkBlocked,
    #[error("firewall blocks incoming connections")]
    FirewallBlocked,
    #[error("device is not paired")]
    NotPaired,
    #[error("pairing rejected")]
    PairingRejected,
    #[error("pairing code expired")]
    PairingExpired,
    #[error("device was removed or blocked")]
    Revoked,
    #[error("peer runs an incompatible version")]
    IncompatibleVersion,
    #[error("permission denied by the other device")]
    PeerDenied,
    #[error("microphone permission denied")]
    MicPermissionDenied,
    #[error("audio device unavailable: {0}")]
    AudioDevice(String),
    #[error("system audio capture unsupported")]
    LoopbackUnsupported,
    #[error("virtual microphone not installed")]
    VirtualMicMissing,
    #[error("route not found")]
    RouteNotFound,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("engine stopped")]
    Stopped,
    #[error("engine is starting")]
    Starting,
    #[error("cancelled")]
    Cancelled,
    #[error("internal error: {0}")]
    Internal(String),
}

impl EngineError {
    /// Stable localization key, e.g. `error.network.unreachable`.
    pub fn key(&self) -> &'static str {
        match self {
            Self::DeviceNotFound => "error.device.notFound",
            Self::Unreachable => "error.network.unreachable",
            Self::NetworkBlocked => "error.network.blocked",
            Self::FirewallBlocked => "error.network.firewall",
            Self::NotPaired => "error.security.notPaired",
            Self::PairingRejected => "error.security.pairingRejected",
            Self::PairingExpired => "error.security.pairingExpired",
            Self::Revoked => "error.security.revoked",
            Self::IncompatibleVersion => "error.compat.version",
            Self::PeerDenied => "error.permission.peerDenied",
            Self::MicPermissionDenied => "error.permission.mic",
            Self::AudioDevice(_) => "error.audio.device",
            Self::LoopbackUnsupported => "error.audio.loopbackUnsupported",
            Self::VirtualMicMissing => "error.audio.virtualMicMissing",
            Self::RouteNotFound => "error.route.notFound",
            Self::InvalidInput(_) => "error.input.invalid",
            Self::Storage(_) => "error.storage",
            Self::Stopped => "error.engine.stopped",
            Self::Starting => "error.engine.starting",
            Self::Cancelled => "error.cancelled",
            Self::Internal(_) => "error.internal",
        }
    }

    pub fn severity(&self) -> Severity {
        match self {
            Self::DeviceNotFound
            | Self::RouteNotFound
            | Self::InvalidInput(_)
            | Self::Cancelled => Severity::Warning,
            _ => Severity::Error,
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Unreachable | Self::NetworkBlocked | Self::FirewallBlocked | Self::AudioDevice(_)
        )
    }

    pub fn fix(&self) -> Option<FixAction> {
        match self {
            Self::Unreachable => Some(FixAction::RetryConnection),
            Self::NetworkBlocked => Some(FixAction::SwitchToUsb),
            Self::FirewallBlocked => Some(FixAction::OpenFirewallFix),
            Self::NotPaired | Self::Revoked | Self::PairingExpired => Some(FixAction::PairAgain),
            Self::IncompatibleVersion => Some(FixAction::UpdatePeerApp),
            Self::MicPermissionDenied => Some(FixAction::OpenMicPermissionSettings),
            Self::AudioDevice(_) => Some(FixAction::ChooseAnotherDevice),
            Self::VirtualMicMissing => Some(FixAction::InstallVirtualMic),
            _ => None,
        }
    }
}

impl From<sp_audio_io::AudioError> for EngineError {
    fn from(e: sp_audio_io::AudioError) -> Self {
        use sp_audio_io::AudioError as A;
        match e {
            A::PermissionDenied => Self::MicPermissionDenied,
            A::LoopbackUnsupported => Self::LoopbackUnsupported,
            other => Self::AudioDevice(other.to_string()),
        }
    }
}

impl From<sp_security::SecurityError> for EngineError {
    fn from(e: sp_security::SecurityError) -> Self {
        match e {
            sp_security::SecurityError::PairingExpired => Self::PairingExpired,
            sp_security::SecurityError::PairingProofMismatch => Self::PairingRejected,
            other => Self::Storage(other.to_string()),
        }
    }
}

impl From<sp_transport::TransportError> for EngineError {
    fn from(e: sp_transport::TransportError) -> Self {
        match e {
            sp_transport::TransportError::Timeout => Self::Unreachable,
            sp_transport::TransportError::Connect(_)
            | sp_transport::TransportError::Connection(_)
            | sp_transport::TransportError::Io(_)
            | sp_transport::TransportError::Closed => Self::Unreachable,
            other => Self::Internal(other.to_string()),
        }
    }
}

/// Serializable error for UIs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorView {
    pub key: &'static str,
    pub message: String,
    pub severity: Severity,
    pub retryable: bool,
    pub fix: Option<FixAction>,
}

impl From<&EngineError> for ErrorView {
    fn from(e: &EngineError) -> Self {
        Self {
            key: e.key(),
            message: e.to_string(),
            severity: e.severity(),
            retryable: e.retryable(),
            fix: e.fix(),
        }
    }
}
