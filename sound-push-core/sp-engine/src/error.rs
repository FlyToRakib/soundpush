//! Engine error taxonomy shared by every platform.
//!
//! Each error maps to a stable message key (localized by the UI), a stable support code, a
//! severity, whether retrying can help, and an optional one-tap fix action.
//!
//! **Codes never change.** `SP-<AREA>-<NNN>` identifies one failure whatever language the app
//! runs in, so a user can quote it and a maintainer can look it up (plan §36.4). They appear in
//! error messages, diagnostics exports and logs, and are documented in `docs/error-codes.md`.
//! A new error takes the next free number in its area; a retired one's number is never reused.

use serde::Serialize;
use sp_protocol::control::StopReason;

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
    #[error("too many pairing attempts; try again in a minute")]
    PairingRateLimited,
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
    /// Stable support code, e.g. `SP-NET-004` (plan §36.4). Documented in `docs/error-codes.md`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unreachable => "SP-NET-001",
            Self::DeviceNotFound => "SP-NET-002",
            Self::FirewallBlocked => "SP-NET-003",
            Self::NetworkBlocked => "SP-NET-004",
            Self::NotPaired => "SP-SEC-001",
            Self::PairingRejected => "SP-SEC-002",
            Self::PairingExpired => "SP-SEC-003",
            Self::PairingRateLimited => "SP-SEC-004",
            Self::Revoked => "SP-SEC-005",
            Self::IncompatibleVersion => "SP-CMP-001",
            Self::PeerDenied => "SP-PRM-001",
            Self::MicPermissionDenied => "SP-PRM-002",
            Self::AudioDevice(_) => "SP-AUD-001",
            Self::LoopbackUnsupported => "SP-AUD-002",
            Self::VirtualMicMissing => "SP-AUD-003",
            Self::RouteNotFound => "SP-CFG-001",
            Self::InvalidInput(_) => "SP-CFG-002",
            Self::Storage(_) => "SP-SYS-001",
            Self::Stopped => "SP-SYS-002",
            Self::Starting => "SP-SYS-003",
            Self::Cancelled => "SP-SYS-004",
            Self::Internal(_) => "SP-SYS-005",
        }
    }

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
            Self::PairingRateLimited => "error.security.pairingRateLimited",
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
            | Self::PairingRateLimited
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

/// Stable support code for a route or session that the peer ended (plan §36.4).
///
/// The number is the reason's wire value, so a code read from a log always maps back to the same
/// `StopReason`. Documented in `docs/error-codes.md`.
pub fn stop_reason_code(reason: StopReason) -> &'static str {
    match reason {
        StopReason::Unspecified => "SP-SES-000",
        StopReason::UserStopped => "SP-SES-001",
        StopReason::PeerStopped => "SP-SES-002",
        StopReason::PermissionDenied => "SP-SES-003",
        StopReason::PermissionRevoked => "SP-SES-004",
        StopReason::DeviceBusy => "SP-SES-005",
        StopReason::AudioDeviceLost => "SP-SES-006",
        StopReason::CallInterruption => "SP-SES-007",
        StopReason::Superseded => "SP-SES-008",
        StopReason::IncompatibleVersion => "SP-SES-009",
        StopReason::NetworkLost => "SP-SES-010",
        StopReason::UnsupportedEndpoint => "SP-SES-011",
        StopReason::Timeout => "SP-SES-012",
        StopReason::RateLimited => "SP-SES-013",
    }
}

/// Serializable error for UIs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorView {
    /// Stable support code, e.g. `SP-NET-004`. Shown next to the message and in diagnostics.
    pub code: &'static str,
    pub key: &'static str,
    pub message: String,
    pub severity: Severity,
    pub retryable: bool,
    pub fix: Option<FixAction>,
}

impl From<&EngineError> for ErrorView {
    fn from(e: &EngineError) -> Self {
        Self {
            code: e.code(),
            key: e.key(),
            message: e.to_string(),
            severity: e.severity(),
            retryable: e.retryable(),
            fix: e.fix(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Codes are public API: a missing, duplicated or reshaped one breaks support links.
    #[test]
    fn every_error_has_its_own_well_formed_code() {
        let errors = [
            EngineError::DeviceNotFound,
            EngineError::Unreachable,
            EngineError::NetworkBlocked,
            EngineError::FirewallBlocked,
            EngineError::NotPaired,
            EngineError::PairingRejected,
            EngineError::PairingExpired,
            EngineError::PairingRateLimited,
            EngineError::Revoked,
            EngineError::IncompatibleVersion,
            EngineError::PeerDenied,
            EngineError::MicPermissionDenied,
            EngineError::AudioDevice(String::new()),
            EngineError::LoopbackUnsupported,
            EngineError::VirtualMicMissing,
            EngineError::RouteNotFound,
            EngineError::InvalidInput(String::new()),
            EngineError::Storage(String::new()),
            EngineError::Stopped,
            EngineError::Starting,
            EngineError::Cancelled,
            EngineError::Internal(String::new()),
        ];
        let mut codes: Vec<&str> = errors.iter().map(EngineError::code).collect();
        let total = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), total, "two errors share a code");
        for code in &codes {
            let parts: Vec<&str> = code.split('-').collect();
            assert_eq!(parts.len(), 3, "{code}");
            assert_eq!(parts[0], "SP");
            assert!(parts[1].len() == 3 && parts[1].bytes().all(|b| b.is_ascii_uppercase()));
            assert!(parts[2].len() == 3 && parts[2].bytes().all(|b| b.is_ascii_digit()));
        }
        assert_eq!(
            ErrorView::from(&EngineError::NetworkBlocked).code,
            "SP-NET-004"
        );
    }

    #[test]
    fn stop_reason_codes_carry_the_wire_value() {
        for reason in [
            StopReason::Unspecified,
            StopReason::UserStopped,
            StopReason::PeerStopped,
            StopReason::PermissionDenied,
            StopReason::PermissionRevoked,
            StopReason::DeviceBusy,
            StopReason::AudioDeviceLost,
            StopReason::CallInterruption,
            StopReason::Superseded,
            StopReason::IncompatibleVersion,
            StopReason::NetworkLost,
            StopReason::UnsupportedEndpoint,
            StopReason::Timeout,
            StopReason::RateLimited,
        ] {
            assert_eq!(
                stop_reason_code(reason),
                format!("SP-SES-{:03}", reason as i32)
            );
        }
    }
}
