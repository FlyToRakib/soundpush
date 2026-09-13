//! Pairing primitives.
//!
//! **QR pairing:** the displaying device shows a compact code with its device ID
//! (128-bit SHA-256 prefix of its public key), a few addresses and a single-use
//! 128-bit secret. The scanning device pins that device ID during the TLS
//! handshake and proves possession of the secret with an HMAC bound to both
//! identities, which authenticates both sides.
//!
//! The code is uppercase base32 so it stays in QR *alphanumeric* mode, which makes
//! the QR symbol small and easy for cameras to read. Nothing security-relevant is
//! dropped: the device name travels in the encrypted handshake, and expiry is
//! enforced by the displaying device (the only side that can accept the secret).
//!
//! **Code pairing:** both devices derive a 6-digit short authentication string
//! (SAS) from the TLS exporter of their shared session and the user confirms the
//! codes match on both screens.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::SecurityError;
use crate::identity::{DeviceId, Fingerprint};

type HmacSha256 = Hmac<Sha256>;

/// Prefix of a SoundPush pairing code.
pub const QR_PREFIX: &str = "SP1:";
/// TLS exporter label for SAS derivation.
pub const SAS_EXPORTER_LABEL: &[u8] = b"EXPORTER-soundpush-pair-sas";
/// How long a QR code stays valid (enforced by the displaying device).
pub const QR_VALIDITY_SECS: u64 = 300;
/// Addresses included in a code. More only makes the code denser.
pub const MAX_QR_ADDRESSES: usize = 3;

const CODE_VERSION: u8 = 1;
const PROOF_LABEL: &[u8] = b"soundpush-pair-v2";

/// Content of a pairing QR code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrPairingPayload {
    pub device_id: DeviceId,
    /// Candidate addresses, best first. All share one port.
    pub addresses: Vec<SocketAddr>,
    pub secret: [u8; 16],
    /// Known only to the displaying device (not encoded); 0 on the scanning side.
    pub expires_unix: u64,
}

impl QrPairingPayload {
    /// Create a code for this device. Picks the addresses a phone can actually dial.
    pub fn new(device_id: DeviceId, addresses: Vec<SocketAddr>, now_unix: u64) -> Self {
        let mut secret = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut secret);
        Self {
            device_id,
            addresses: select_addresses(addresses),
            secret,
            expires_unix: now_unix + QR_VALIDITY_SECS,
        }
    }

    pub fn is_expired(&self, now_unix: u64) -> bool {
        now_unix > self.expires_unix
    }

    /// Encode as `SP1:<BASE32>`:
    /// `version u8 | device_id [16] | secret [16] | port u16 | count u8 | (4 + ipv4 [4] | 6 + ipv6 [16])*`
    pub fn to_uri(&self) -> String {
        let port = self.addresses.first().map(SocketAddr::port).unwrap_or(0);
        let mut bytes = Vec::with_capacity(36 + MAX_QR_ADDRESSES * 17);
        bytes.push(CODE_VERSION);
        bytes.extend_from_slice(&self.device_id.0);
        bytes.extend_from_slice(&self.secret);
        bytes.extend_from_slice(&port.to_be_bytes());
        let addrs: Vec<IpAddr> = self
            .addresses
            .iter()
            .filter(|a| a.port() == port)
            .map(SocketAddr::ip)
            .take(MAX_QR_ADDRESSES)
            .collect();
        bytes.push(addrs.len() as u8);
        for ip in addrs {
            match ip {
                IpAddr::V4(v4) => {
                    bytes.push(4);
                    bytes.extend_from_slice(&v4.octets());
                }
                IpAddr::V6(v6) => {
                    bytes.push(6);
                    bytes.extend_from_slice(&v6.octets());
                }
            }
        }
        format!("{QR_PREFIX}{}", base32_encode(&bytes))
    }

    /// Parse an untrusted scanned code. Never panics.
    pub fn from_uri(text: &str) -> Result<Self, SecurityError> {
        let text = text.trim();
        if text.len() > 256 {
            return Err(SecurityError::InvalidPairingPayload("too long"));
        }
        let body = text
            .strip_prefix(QR_PREFIX)
            .ok_or(SecurityError::InvalidPairingPayload("not a SoundPush pairing code"))?;
        let bytes = base32_decode(body).ok_or(SecurityError::InvalidPairingPayload("encoding"))?;

        let mut r = Reader { buf: &bytes, pos: 0 };
        if r.u8()? != CODE_VERSION {
            return Err(SecurityError::InvalidPairingPayload("unsupported version"));
        }
        let device_id = DeviceId(r.array()?);
        let secret: [u8; 16] = r.array()?;
        let port = u16::from_be_bytes(r.array()?);
        let count = r.u8()? as usize;
        if count == 0 || count > MAX_QR_ADDRESSES || port == 0 {
            return Err(SecurityError::InvalidPairingPayload("addresses"));
        }
        let mut addresses = Vec::with_capacity(count);
        for _ in 0..count {
            let ip = match r.u8()? {
                4 => IpAddr::V4(Ipv4Addr::from(r.array::<4>()?)),
                6 => IpAddr::V6(Ipv6Addr::from(r.array::<16>()?)),
                _ => return Err(SecurityError::InvalidPairingPayload("address type")),
            };
            addresses.push(SocketAddr::new(ip, port));
        }
        if r.pos != bytes.len() {
            return Err(SecurityError::InvalidPairingPayload("trailing data"));
        }
        Ok(Self {
            device_id,
            addresses,
            secret,
            expires_unix: 0,
        })
    }
}

/// Keep addresses a phone can dial from a QR code: private/public IPv4 first, then routable
/// IPv6. Link-local IPv6 needs an interface scope that can't be carried in a code. Loopback
/// is kept only when nothing else exists (single-machine tests).
fn select_addresses(addresses: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let port = addresses.first().map(SocketAddr::port).unwrap_or(0);
    let same_port = addresses.iter().filter(|a| a.port() == port).copied();
    let mut v4: Vec<SocketAddr> = Vec::new();
    let mut v6: Vec<SocketAddr> = Vec::new();
    let mut loopback: Vec<SocketAddr> = Vec::new();
    for addr in same_port {
        match addr.ip() {
            IpAddr::V4(ip) if ip.is_loopback() => loopback.push(addr),
            IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_link_local() => v4.push(addr),
            IpAddr::V6(ip) if ip.is_loopback() => loopback.push(addr),
            IpAddr::V6(ip) if !ip.is_unspecified() && (ip.segments()[0] & 0xffc0) != 0xfe80 => v6.push(addr),
            _ => {}
        }
    }
    let mut out: Vec<SocketAddr> = v4.into_iter().chain(v6).collect();
    if out.is_empty() {
        out = loopback;
    }
    out.dedup();
    out.truncate(MAX_QR_ADDRESSES);
    out
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, SecurityError> {
        Ok(self.array::<1>()?[0])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], SecurityError> {
        let end = self.pos + N;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or(SecurityError::InvalidPairingPayload("truncated"))?;
        self.pos = end;
        slice.try_into().map_err(|_| SecurityError::InvalidPairingPayload("truncated"))
    }
}

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32, uppercase, no padding (all characters are QR-alphanumeric).
fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buffer >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

fn proof_mac(secret: &[u8; 16], scanner: &Fingerprint, displayer: &DeviceId) -> HmacSha256 {
    // HMAC accepts keys of any length; this cannot fail.
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret).unwrap_or_else(|_| unreachable!());
    mac.update(PROOF_LABEL);
    mac.update(&scanner.0);
    mac.update(&displayer.0);
    mac
}

/// Proof that the scanner saw the QR secret, bound to both identities.
pub fn pairing_proof(secret: &[u8; 16], scanner: &Fingerprint, displayer: &DeviceId) -> [u8; 32] {
    proof_mac(secret, scanner, displayer).finalize().into_bytes().into()
}

/// Constant-time verification of a pairing proof.
pub fn verify_pairing_proof(
    secret: &[u8; 16],
    scanner: &Fingerprint,
    displayer: &DeviceId,
    proof: &[u8],
) -> Result<(), SecurityError> {
    proof_mac(secret, scanner, displayer)
        .verify_slice(proof)
        .map_err(|_| SecurityError::PairingProofMismatch)
}

/// Derive the 6-digit short authentication string from TLS exporter output.
/// Both sides must pass fingerprints in the same (sorted) order.
pub fn sas_code(exporter_secret: &[u8], a: &Fingerprint, b: &Fingerprint) -> String {
    let (first, second) = if a.0 <= b.0 { (a, b) } else { (b, a) };
    let mut h = Sha256::new();
    h.update(exporter_secret);
    h.update(first.0);
    h.update(second.0);
    let digest = h.finalize();
    let n = u64::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ]);
    format!("{:06}", n % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceIdentity;

    const QR_ALPHANUMERIC: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

    #[test]
    fn qr_roundtrip_is_compact_and_alphanumeric() {
        let id = DeviceIdentity::generate();
        let payload = QrPairingPayload::new(
            id.device_id(),
            vec![
                "[fe80::1]:47650".parse().unwrap(),
                "192.168.1.20:47650".parse().unwrap(),
                "10.0.0.5:47650".parse().unwrap(),
                "172.20.1.2:47650".parse().unwrap(),
                "192.168.9.9:47650".parse().unwrap(),
            ],
            1_000,
        );
        let uri = payload.to_uri();
        assert!(uri.chars().all(|c| QR_ALPHANUMERIC.contains(c)), "{uri}");
        // QR version 4 (33×33 modules) at error-correction level M holds 90 alphanumeric characters.
        assert!(uri.len() <= 90, "code too long for a version-4 QR: {} chars", uri.len());

        let parsed = QrPairingPayload::from_uri(&uri).unwrap();
        assert_eq!(parsed.device_id, payload.device_id);
        assert_eq!(parsed.secret, payload.secret);
        // Link-local dropped, capped at three, best first.
        assert_eq!(
            parsed.addresses,
            vec![
                "192.168.1.20:47650".parse::<SocketAddr>().unwrap(),
                "10.0.0.5:47650".parse().unwrap(),
                "172.20.1.2:47650".parse().unwrap(),
            ]
        );
        assert!(!payload.is_expired(1_000));
        assert!(payload.is_expired(1_000 + QR_VALIDITY_SECS + 1));
    }

    #[test]
    fn loopback_kept_only_when_nothing_else() {
        let id = DeviceIdentity::generate().device_id();
        let only_loopback = QrPairingPayload::new(id, vec!["127.0.0.1:5000".parse().unwrap()], 0);
        assert_eq!(only_loopback.addresses.len(), 1);
        let mixed = QrPairingPayload::new(id, vec!["127.0.0.1:5000".parse().unwrap(), "192.168.0.2:5000".parse().unwrap()], 0);
        assert_eq!(mixed.addresses, vec!["192.168.0.2:5000".parse::<SocketAddr>().unwrap()]);
    }

    #[test]
    fn qr_rejects_garbage() {
        assert!(QrPairingPayload::from_uri("https://evil.example").is_err());
        assert!(QrPairingPayload::from_uri("SP1:").is_err());
        assert!(QrPairingPayload::from_uri("SP1:!!!").is_err());
        assert!(QrPairingPayload::from_uri("SP1:AEAQCAIBAEAQCAIB").is_err());
        let id = DeviceIdentity::generate().device_id();
        let valid = QrPairingPayload::new(id, vec!["192.168.1.2:1".parse().unwrap()], 0).to_uri();
        assert!(QrPairingPayload::from_uri(&format!("{valid}AA")).is_err(), "trailing data");
        assert!(QrPairingPayload::from_uri(&valid[..valid.len() - 4]).is_err(), "truncated");
    }

    #[test]
    fn base32_roundtrip() {
        for len in 0..40 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base32_decode(&base32_encode(&data)).unwrap(), data);
        }
    }

    #[test]
    fn proof_verifies_only_with_right_secret_and_identities() {
        let a = DeviceIdentity::generate();
        let b = DeviceIdentity::generate();
        let secret = [9u8; 16];
        let proof = pairing_proof(&secret, &a.fingerprint(), &b.device_id());
        assert!(verify_pairing_proof(&secret, &a.fingerprint(), &b.device_id(), &proof).is_ok());
        assert!(verify_pairing_proof(&[8u8; 16], &a.fingerprint(), &b.device_id(), &proof).is_err());
        assert!(verify_pairing_proof(&secret, &b.fingerprint(), &a.device_id(), &proof).is_err());
    }

    #[test]
    fn sas_is_symmetric_and_six_digits() {
        let a = DeviceIdentity::generate().fingerprint();
        let b = DeviceIdentity::generate().fingerprint();
        let code1 = sas_code(b"shared", &a, &b);
        let code2 = sas_code(b"shared", &b, &a);
        assert_eq!(code1, code2);
        assert_eq!(code1.len(), 6);
        assert_ne!(code1, sas_code(b"other", &a, &b));
    }
}
