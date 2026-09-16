//! TLS configuration shared by the QUIC and TCP transports.
//!
//! Both transports present the same self-signed identity certificate and use the same
//! verifiers, so pinning and trust behave identically whichever path a connection takes.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sp_security::{DeviceId, DeviceIdentity};

use crate::TransportError;
use crate::verifier::{PeerClientVerifier, PeerServerVerifier};

/// Name sent in SNI; certificates are verified by key, never by name.
pub(crate) const SERVER_NAME: &str = "soundpush.local";

pub(crate) struct Credentials {
    pub provider: Arc<CryptoProvider>,
    cert: CertificateDer<'static>,
    key: Arc<Vec<u8>>,
    /// This device's id, so a connection that reaches this very device is recognised as one.
    own: DeviceId,
}

impl Credentials {
    pub fn new(identity: &DeviceIdentity) -> Result<Self, TransportError> {
        let cert = identity.certificate()?;
        Ok(Self {
            provider: Arc::new(rustls::crypto::ring::default_provider()),
            cert: CertificateDer::from(cert.cert_der.clone()),
            key: Arc::new(cert.key_pkcs8_der.to_vec()),
            own: identity.device_id(),
        })
    }

    /// TLS 1.3 server with mandatory client certificates.
    pub fn server_config(&self, alpn: &[u8]) -> Result<rustls::ServerConfig, TransportError> {
        let mut config = rustls::ServerConfig::builder_with_provider(self.provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| TransportError::Tls(e.to_string()))?
            .with_client_cert_verifier(PeerClientVerifier::new(&self.provider))
            .with_single_cert(vec![self.cert.clone()], private_key(&self.key))
            .map_err(|e| TransportError::Tls(e.to_string()))?;
        config.alpn_protocols = vec![alpn.to_vec()];
        Ok(config)
    }

    /// TLS 1.3 client presenting our certificate; with `pinned`, the handshake fails unless the
    /// server's key has that device ID.
    /// `dialed_self` is raised when the certificate on the other end turns out to be this
    /// device's own, which means the address dialled is one of ours (see [`PeerServerVerifier`]).
    pub fn client_config(
        &self,
        pinned: Option<DeviceId>,
        alpn: &[u8],
        dialed_self: Arc<AtomicBool>,
    ) -> Result<rustls::ClientConfig, TransportError> {
        let mut config = rustls::ClientConfig::builder_with_provider(self.provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| TransportError::Tls(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(PeerServerVerifier::new(
                &self.provider,
                pinned,
                self.own,
                dialed_self,
            ))
            .with_client_auth_cert(vec![self.cert.clone()], private_key(&self.key))
            .map_err(|e| TransportError::Tls(e.to_string()))?;
        config.alpn_protocols = vec![alpn.to_vec()];
        Ok(config)
    }
}

fn private_key(der: &[u8]) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der.to_vec()))
}
