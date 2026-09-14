//! Merges adverts from all sources by device ID and expires stale peers.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sp_security::DeviceId;

use crate::PeerAdvert;
use crate::candidates::order;

/// A peer is considered gone when no advert arrived for this long.
pub const PEER_TTL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    /// New peer, or an existing peer whose details changed.
    Updated(PeerAdvert),
    Lost(DeviceId),
}

struct Entry {
    advert: PeerAdvert,
    last_seen: Instant,
}

#[derive(Default)]
pub struct PeerRegistry {
    peers: HashMap<DeviceId, Entry>,
    local: Option<DeviceId>,
}

impl PeerRegistry {
    pub fn new(local: DeviceId) -> Self {
        Self {
            peers: HashMap::new(),
            local: Some(local),
        }
    }

    /// Merge an advert. Returns an event when something visible changed.
    pub fn ingest(&mut self, mut advert: PeerAdvert, now: Instant) -> Option<DiscoveryEvent> {
        if Some(advert.device_id) == self.local {
            return None;
        }
        match self.peers.get_mut(&advert.device_id) {
            Some(entry) => {
                entry.last_seen = now;
                let mut merged = entry.advert.clone();
                let mut addrs = merged.addresses.clone();
                addrs.extend(advert.addresses.iter().copied());
                merged.addresses = order(addrs);
                if !advert.name.is_empty() {
                    merged.name = advert.name;
                }
                if !advert.platform.is_empty() {
                    merged.platform = advert.platform;
                }
                merged.public_key = merged.public_key.or(advert.public_key);
                merged.capabilities = advert.capabilities;
                merged.protocol_max = advert.protocol_max.max(merged.protocol_max);
                if merged != entry.advert {
                    entry.advert = merged.clone();
                    Some(DiscoveryEvent::Updated(merged))
                } else {
                    None
                }
            }
            None => {
                advert.addresses = order(advert.addresses);
                self.peers.insert(
                    advert.device_id,
                    Entry {
                        advert: advert.clone(),
                        last_seen: now,
                    },
                );
                Some(DiscoveryEvent::Updated(advert))
            }
        }
    }

    /// Remove a peer immediately (e.g. mDNS goodbye).
    pub fn remove(&mut self, id: &DeviceId) -> Option<DiscoveryEvent> {
        self.peers.remove(id).map(|_| DiscoveryEvent::Lost(*id))
    }

    /// Expire peers not seen within [`PEER_TTL`].
    pub fn sweep(&mut self, now: Instant) -> Vec<DiscoveryEvent> {
        let stale: Vec<DeviceId> = self
            .peers
            .iter()
            .filter(|(_, e)| now.duration_since(e.last_seen) > PEER_TTL)
            .map(|(id, _)| *id)
            .collect();
        stale
            .into_iter()
            .filter_map(|id| self.remove(&id))
            .collect()
    }

    pub fn get(&self, id: &DeviceId) -> Option<&PeerAdvert> {
        self.peers.get(id).map(|e| &e.advert)
    }

    pub fn peers(&self) -> impl Iterator<Item = &PeerAdvert> {
        self.peers.values().map(|e| &e.advert)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;
    use sp_protocol::Capabilities;
    use sp_security::DeviceIdentity;

    fn advert(id: DeviceId, addr: &str, name: &str, source: Source) -> PeerAdvert {
        PeerAdvert {
            device_id: id,
            public_key: None,
            name: name.into(),
            platform: String::new(),
            addresses: vec![addr.parse().unwrap()],
            capabilities: Capabilities::default(),
            protocol_max: 0x0001_0000,
            source,
        }
    }

    #[test]
    fn merges_sources_and_ignores_self() {
        let me = DeviceIdentity::generate().device_id();
        let peer = DeviceIdentity::generate().device_id();
        let mut reg = PeerRegistry::new(me);
        let t = Instant::now();

        assert!(
            reg.ingest(advert(me, "192.168.1.2:1", "me", Source::Mdns), t)
                .is_none()
        );
        assert!(
            reg.ingest(advert(peer, "192.168.1.3:47650", "Phone", Source::Mdns), t)
                .is_some()
        );
        // Same data again: no event.
        assert!(
            reg.ingest(advert(peer, "192.168.1.3:47650", "", Source::Beacon), t)
                .is_none()
        );
        // New address merges.
        let ev = reg
            .ingest(advert(peer, "[fe80::5]:47650", "", Source::Beacon), t)
            .unwrap();
        match ev {
            DiscoveryEvent::Updated(a) => {
                assert_eq!(a.addresses.len(), 2);
                assert_eq!(a.name, "Phone");
            }
            DiscoveryEvent::Lost(_) => panic!("unexpected"),
        }
    }

    #[test]
    fn stale_peers_expire() {
        let mut reg = PeerRegistry::new(DeviceIdentity::generate().device_id());
        let peer = DeviceIdentity::generate().device_id();
        let t = Instant::now();
        reg.ingest(advert(peer, "10.0.0.2:1", "x", Source::Beacon), t);
        assert!(reg.sweep(t + Duration::from_secs(5)).is_empty());
        assert_eq!(
            reg.sweep(t + PEER_TTL + Duration::from_secs(1)),
            vec![DiscoveryEvent::Lost(peer)]
        );
        assert_eq!(reg.peers().count(), 0);
    }
}
