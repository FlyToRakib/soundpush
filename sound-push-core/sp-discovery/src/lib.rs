//! Device discovery.
//!
//! Three mechanisms feed one [`registry::PeerRegistry`], merged by device ID:
//! - mDNS / DNS-SD (`_soundpush._udp.local.`),
//! - signed UDP broadcast beacons (for networks that drop multicast),
//! - externally supplied adverts (Android `NsdManager`, QR codes, manual entry).
//!
//! Discovery results are *hints*. Nothing here grants trust; the engine still
//! authenticates every connection against the trust store.
#![forbid(unsafe_code)]

pub mod beacon;
pub mod candidates;
pub mod mdns;
pub mod registry;
pub mod service;

use std::net::SocketAddr;

use sp_protocol::Capabilities;
use sp_security::DeviceId;

pub use registry::{DiscoveryEvent, PeerRegistry};
pub use service::{Discovery, DiscoveryConfig, Visibility};

/// mDNS service type.
pub const SERVICE_TYPE: &str = "_soundpush._udp.local.";
/// UDP port for broadcast beacons.
pub const BEACON_PORT: u16 = 47651;

/// How an advert was learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Mdns,
    Beacon,
    External,
}

/// A discovered peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAdvert {
    pub device_id: DeviceId,
    /// Present when the advert carried a verifiable public key (beacons).
    pub public_key: Option<[u8; 32]>,
    /// Empty when the peer hides its name.
    pub name: String,
    pub platform: String,
    pub addresses: Vec<SocketAddr>,
    pub capabilities: Capabilities,
    pub protocol_max: u32,
    pub source: Source,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("mdns error: {0}")]
    Mdns(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed beacon")]
    MalformedBeacon,
    #[error("beacon signature invalid")]
    BadSignature,
}
