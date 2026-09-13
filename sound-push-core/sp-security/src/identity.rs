//! Device identity.
//!
//! Each installation owns one Ed25519 key. Trust is bound to the key, never to
//! names or addresses. The TLS certificate is derived from the key on demand.

use std::fmt;

use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::SecurityError;

/// SHA-256 of the device public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    pub fn of_public_key(public_key: &[u8; 32]) -> Self {
        Self(Sha256::digest(public_key).into())
    }

    pub fn device_id(&self) -> DeviceId {
        let mut id = [0u8; 16];
        id.copy_from_slice(&self.0[..16]);
        DeviceId(id)
    }

    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({}…)", &self.to_hex()[..12])
    }
}

/// Stable, public device identifier: first 16 bytes of the fingerprint.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceId(pub [u8; 16]);

impl DeviceId {
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = unhex(s)?;
        let arr: [u8; 16] = bytes.try_into().ok()?;
        Some(Self(arr))
    }

    /// Human-friendly form used in support and diagnostics: `SP-XXXX-XXXX-XXXX`.
    pub fn display_code(&self) -> String {
        let b32 = crockford_base32(&self.0[..8]);
        format!("SP-{}-{}-{}", &b32[0..4], &b32[4..8], &b32[8..12])
    }

    /// Short prefix safe for logs.
    pub fn short(&self) -> String {
        self.to_hex()[..8].to_string()
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({})", self.short())
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display_code())
    }
}

/// DER-encoded self-signed certificate and its PKCS#8 private key.
pub struct SelfSignedCert {
    pub cert_der: Vec<u8>,
    pub key_pkcs8_der: Zeroizing<Vec<u8>>,
}

/// The local device's identity keypair.
pub struct DeviceIdentity {
    signing: SigningKey,
}

impl DeviceIdentity {
    pub fn generate() -> Self {
        let mut rng = rand::rngs::OsRng;
        Self {
            signing: SigningKey::generate(&mut rng),
        }
    }

    pub fn from_secret_bytes(secret: &[u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(secret),
        }
    }

    pub fn secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.signing.to_bytes())
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of_public_key(&self.public_key())
    }

    pub fn device_id(&self) -> DeviceId {
        self.fingerprint().device_id()
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing.sign(message).to_bytes()
    }

    /// Generate a TLS certificate bound to this identity key.
    pub fn certificate(&self) -> Result<SelfSignedCert, SecurityError> {
        let pkcs8 = self
            .signing
            .to_pkcs8_der()
            .map_err(|e| SecurityError::Certificate(e.to_string()))?;
        let key_der = Zeroizing::new(pkcs8.as_bytes().to_vec());

        let key_pair = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(&key_der.as_slice().into(), &rcgen::PKCS_ED25519)
            .map_err(|e| SecurityError::Certificate(e.to_string()))?;

        let mut params = rcgen::CertificateParams::new(vec!["soundpush.local".to_string()])
            .map_err(|e| SecurityError::Certificate(e.to_string()))?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, format!("SoundPush {}", self.device_id().short()));
        params.not_before = rcgen::date_time_ymd(2024, 1, 1);
        params.not_after = rcgen::date_time_ymd(2124, 1, 1);

        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| SecurityError::Certificate(e.to_string()))?;
        Ok(SelfSignedCert {
            cert_der: cert.der().to_vec(),
            key_pkcs8_der: key_der,
        })
    }
}

/// Verify a signature made by `public_key`.
pub fn verify_signature(public_key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    key.verify(message, &Signature::from_bytes(signature)).is_ok()
}

/// Extract the Ed25519 public key from a peer's DER certificate.
pub fn public_key_from_cert(cert_der: &[u8]) -> Result<[u8; 32], SecurityError> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der)
        .map_err(|e| SecurityError::InvalidCertificate(e.to_string()))?;
    let spki = cert.public_key();
    // OID 1.3.101.112 = Ed25519
    if spki.algorithm.algorithm.to_id_string() != "1.3.101.112" {
        return Err(SecurityError::InvalidCertificate("not an Ed25519 key".into()));
    }
    let raw = spki.subject_public_key.data.as_ref();
    raw.try_into()
        .map_err(|_| SecurityError::InvalidCertificate("bad key length".into()))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0F) as usize] as char);
    }
    s
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn crockford_base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_roundtrip_and_ids() {
        let id = DeviceIdentity::generate();
        let restored = DeviceIdentity::from_secret_bytes(&id.secret_bytes());
        assert_eq!(id.public_key(), restored.public_key());
        assert_eq!(id.device_id(), restored.device_id());
        let hex = id.device_id().to_hex();
        assert_eq!(DeviceId::from_hex(&hex), Some(id.device_id()));
        assert!(id.device_id().display_code().starts_with("SP-"));
        assert_eq!(id.device_id().display_code().len(), 17);
    }

    #[test]
    fn certificate_carries_identity_key() {
        let id = DeviceIdentity::generate();
        let cert = id.certificate().unwrap();
        assert_eq!(public_key_from_cert(&cert.cert_der).unwrap(), id.public_key());
    }

    #[test]
    fn signatures_verify() {
        let id = DeviceIdentity::generate();
        let sig = id.sign(b"hello");
        assert!(verify_signature(&id.public_key(), b"hello", &sig));
        assert!(!verify_signature(&id.public_key(), b"hellO", &sig));
    }
}
