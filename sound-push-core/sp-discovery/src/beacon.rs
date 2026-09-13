//! Signed UDP broadcast beacon.
//!
//! ```text
//! magic "SPB1" (4) | public_key (32) | port u16 | capabilities u64 | protocol_max u32
//! | flags u8 | platform_len u8 | platform | name_len u8 | name | signature (64)
//! ```
//! The signature covers every preceding byte. A valid signature proves the sender
//! holds the key for the advertised device ID; it does not make the device trusted.

use sp_protocol::Capabilities;
use sp_security::identity::verify_signature;
use sp_security::{DeviceIdentity, Fingerprint};

use crate::DiscoveryError;

const MAGIC: &[u8; 4] = b"SPB1";
const MAX_NAME: usize = 64;
const MAX_PLATFORM: usize = 16;
pub const MAX_BEACON_LEN: usize = 4 + 32 + 2 + 8 + 4 + 1 + 1 + MAX_PLATFORM + 1 + MAX_NAME + 64;

pub const FLAG_NAME_HIDDEN: u8 = 0b0000_0001;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beacon {
    pub public_key: [u8; 32],
    pub port: u16,
    pub capabilities: Capabilities,
    pub protocol_max: u32,
    pub flags: u8,
    pub platform: String,
    pub name: String,
}

impl Beacon {
    pub fn encode(&self, identity: &DeviceIdentity) -> Vec<u8> {
        let name = truncate_utf8(if self.flags & FLAG_NAME_HIDDEN != 0 { "" } else { &self.name }, MAX_NAME);
        let platform = truncate_utf8(&self.platform, MAX_PLATFORM);
        let mut out = Vec::with_capacity(MAX_BEACON_LEN);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&identity.public_key());
        out.extend_from_slice(&self.port.to_be_bytes());
        out.extend_from_slice(&self.capabilities.0.to_be_bytes());
        out.extend_from_slice(&self.protocol_max.to_be_bytes());
        out.push(self.flags);
        out.push(platform.len() as u8);
        out.extend_from_slice(platform.as_bytes());
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        let sig = identity.sign(&out);
        out.extend_from_slice(&sig);
        out
    }

    /// Parse and verify an untrusted beacon. Never panics.
    pub fn decode(data: &[u8]) -> Result<Self, DiscoveryError> {
        if data.len() < 4 + 32 + 2 + 8 + 4 + 1 + 1 + 1 + 64 || data.len() > MAX_BEACON_LEN {
            return Err(DiscoveryError::MalformedBeacon);
        }
        let (body, sig) = data.split_at(data.len() - 64);
        if &body[..4] != MAGIC {
            return Err(DiscoveryError::MalformedBeacon);
        }
        let mut r = Reader { buf: body, pos: 4 };
        let public_key: [u8; 32] = r.take(32)?.try_into().map_err(|_| DiscoveryError::MalformedBeacon)?;
        let port = u16::from_be_bytes(r.array()?);
        let capabilities = Capabilities(u64::from_be_bytes(r.array()?));
        let protocol_max = u32::from_be_bytes(r.array()?);
        let flags = r.u8()?;
        let platform_len = r.u8()? as usize;
        let platform = r.string(platform_len.min(MAX_PLATFORM))?;
        let name_len = r.u8()? as usize;
        let name = r.string(name_len.min(MAX_NAME))?;
        if r.pos != body.len() {
            return Err(DiscoveryError::MalformedBeacon);
        }
        let sig: [u8; 64] = sig.try_into().map_err(|_| DiscoveryError::MalformedBeacon)?;
        if !verify_signature(&public_key, body, &sig) {
            return Err(DiscoveryError::BadSignature);
        }
        Ok(Self {
            public_key,
            port,
            capabilities,
            protocol_max,
            flags,
            platform,
            name,
        })
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of_public_key(&self.public_key)
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DiscoveryError> {
        let end = self.pos.checked_add(n).ok_or(DiscoveryError::MalformedBeacon)?;
        let slice = self.buf.get(self.pos..end).ok_or(DiscoveryError::MalformedBeacon)?;
        self.pos = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DiscoveryError> {
        self.take(N)?.try_into().map_err(|_| DiscoveryError::MalformedBeacon)
    }

    fn u8(&mut self) -> Result<u8, DiscoveryError> {
        Ok(self.take(1)?[0])
    }

    fn string(&mut self, n: usize) -> Result<String, DiscoveryError> {
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| DiscoveryError::MalformedBeacon)
    }
}

fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beacon(name: &str, flags: u8) -> Beacon {
        Beacon {
            public_key: [0; 32],
            port: 47650,
            capabilities: Capabilities(Capabilities::SINK_SPEAKER),
            protocol_max: 0x0001_0000,
            flags,
            platform: "windows".into(),
            name: name.into(),
        }
    }

    #[test]
    fn roundtrip_and_signature() {
        let id = DeviceIdentity::generate();
        let wire = beacon("Desk PC", 0).encode(&id);
        let decoded = Beacon::decode(&wire).unwrap();
        assert_eq!(decoded.public_key, id.public_key());
        assert_eq!(decoded.name, "Desk PC");
        assert_eq!(decoded.port, 47650);
        assert_eq!(decoded.fingerprint(), id.fingerprint());
    }

    #[test]
    fn hidden_name_is_not_sent() {
        let id = DeviceIdentity::generate();
        let wire = beacon("Secret", FLAG_NAME_HIDDEN).encode(&id);
        assert!(Beacon::decode(&wire).unwrap().name.is_empty());
    }

    #[test]
    fn tampering_is_detected() {
        let id = DeviceIdentity::generate();
        let mut wire = beacon("Desk", 0).encode(&id);
        wire[37] ^= 0xFF; // port byte
        assert!(matches!(Beacon::decode(&wire), Err(DiscoveryError::BadSignature)));
    }

    #[test]
    fn long_names_truncate_on_char_boundary() {
        let id = DeviceIdentity::generate();
        let long = "é".repeat(100);
        let wire = beacon(&long, 0).encode(&id);
        let decoded = Beacon::decode(&wire).unwrap();
        assert!(decoded.name.len() <= MAX_NAME);
    }

    #[test]
    fn garbage_never_panics() {
        for len in 0..400 {
            let data: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            let _ = Beacon::decode(&data);
        }
    }
}
