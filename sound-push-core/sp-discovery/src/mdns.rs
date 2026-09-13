//! mDNS / DNS-SD advertising and browsing.

use std::collections::HashMap;
use std::net::SocketAddr;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use sp_protocol::Capabilities;
use sp_security::DeviceId;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{DiscoveryError, PeerAdvert, SERVICE_TYPE, Source};

pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    fullname: Option<String>,
}

/// What to publish in TXT records.
pub struct MdnsRecord<'a> {
    pub device_id: DeviceId,
    pub name: Option<&'a str>,
    pub platform: &'a str,
    pub port: u16,
    pub capabilities: Capabilities,
    pub protocol_max: u32,
}

impl MdnsAdvertiser {
    pub fn new() -> Result<Self, DiscoveryError> {
        let daemon = ServiceDaemon::new().map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        Ok(Self { daemon, fullname: None })
    }

    pub fn daemon(&self) -> &ServiceDaemon {
        &self.daemon
    }

    /// Publish (or re-publish) this device.
    pub fn publish(&mut self, record: &MdnsRecord<'_>) -> Result<(), DiscoveryError> {
        self.unpublish();
        let instance = record.device_id.to_hex();
        let host = format!("sp-{}.local.", record.device_id.short());
        let mut props = HashMap::new();
        props.insert("v".to_string(), record.protocol_max.to_string());
        props.insert("id".to_string(), record.device_id.to_hex());
        props.insert("c".to_string(), record.capabilities.0.to_string());
        props.insert("pl".to_string(), record.platform.to_string());
        if let Some(name) = record.name {
            props.insert("n".to_string(), name.chars().take(64).collect());
        }
        let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", record.port, Some(props))
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?
            .enable_addr_auto();
        self.fullname = Some(info.get_fullname().to_string());
        self.daemon
            .register(info)
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))
    }

    pub fn unpublish(&mut self) {
        if let Some(fullname) = self.fullname.take() {
            let _ = self.daemon.unregister(&fullname);
        }
    }

    /// Browse for peers, forwarding adverts and removals.
    pub fn browse(
        &self,
        found: mpsc::UnboundedSender<PeerAdvert>,
        lost: mpsc::UnboundedSender<DeviceId>,
    ) -> Result<(), DiscoveryError> {
        let receiver = self
            .daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        tokio::spawn(async move {
            while let Ok(event) = receiver.recv_async().await {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        if let Some(advert) = advert_from_info(&info) {
                            if found.send(advert).is_err() {
                                break;
                            }
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        if let Some(id) = fullname.split('.').next().and_then(DeviceId::from_hex) {
                            let _ = lost.send(id);
                        }
                    }
                    other => debug!(?other, "mdns event"),
                }
            }
        });
        Ok(())
    }
}

impl Drop for MdnsAdvertiser {
    fn drop(&mut self) {
        self.unpublish();
        if let Err(e) = self.daemon.shutdown() {
            warn!(error = %e, "mdns shutdown");
        }
    }
}

fn advert_from_info(info: &ServiceInfo) -> Option<PeerAdvert> {
    let device_id = DeviceId::from_hex(info.get_property_val_str("id")?)?;
    let port = info.get_port();
    let addresses: Vec<SocketAddr> = info
        .get_addresses()
        .iter()
        .map(|ip| SocketAddr::new(*ip, port))
        .collect();
    Some(PeerAdvert {
        device_id,
        public_key: None,
        name: info.get_property_val_str("n").unwrap_or_default().to_string(),
        platform: info.get_property_val_str("pl").unwrap_or_default().to_string(),
        addresses,
        capabilities: Capabilities(info.get_property_val_str("c").and_then(|v| v.parse().ok()).unwrap_or(0)),
        protocol_max: info.get_property_val_str("v").and_then(|v| v.parse().ok()).unwrap_or(0),
        source: Source::Mdns,
    })
}
