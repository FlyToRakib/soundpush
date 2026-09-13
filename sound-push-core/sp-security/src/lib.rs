//! SoundPush security primitives.
//!
//! - [`identity`]: per-install Ed25519 identity, device ID, self-signed certificate.
//! - [`pairing`]: QR pairing payload, pairing proof, short authentication string.
//! - [`trust`]: encrypted trust store of paired devices.
//! - [`permissions`]: per-device permission policy.
#![forbid(unsafe_code)]

pub mod identity;
pub mod pairing;
pub mod permissions;
pub mod secretbox;
pub mod trust;

pub use identity::{DeviceId, DeviceIdentity, Fingerprint, SelfSignedCert, public_key_from_cert};
pub use permissions::{PermissionKind, Permissions, Policy};
pub use trust::{TrustStore, TrustedDevice};

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("certificate generation failed: {0}")]
    Certificate(String),
    #[error("invalid certificate: {0}")]
    InvalidCertificate(String),
    #[error("invalid pairing payload: {0}")]
    InvalidPairingPayload(&'static str),
    #[error("pairing code expired")]
    PairingExpired,
    #[error("pairing proof mismatch")]
    PairingProofMismatch,
    #[error("stored data is corrupted or was encrypted with a different key")]
    Corrupted,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
