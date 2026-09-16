//! Certificate verifiers.
//!
//! There is no PKI: each device's certificate is self-signed with its identity
//! key. The verifiers therefore check that the certificate is a well-formed
//! Ed25519 certificate (and, for the client, optionally that it matches a pinned
//! fingerprint) and delegate handshake signature checks to rustls. Trust
//! decisions happen after the handshake in the engine.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{
    CryptoProvider, WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature,
};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};
use sp_security::{DeviceId, Fingerprint, public_key_from_cert};

fn algorithms(provider: &CryptoProvider) -> WebPkiSupportedAlgorithms {
    provider.signature_verification_algorithms
}

/// The device id a certificate belongs to, or `None` when it is not one of ours at all.
fn device_id_of(cert: &CertificateDer<'_>) -> Option<DeviceId> {
    let key = public_key_from_cert(cert.as_ref()).ok()?;
    Some(Fingerprint::of_public_key(&key).device_id())
}

fn check_cert(cert: &CertificateDer<'_>, pinned: Option<DeviceId>) -> Result<(), Error> {
    let key = public_key_from_cert(cert.as_ref())
        .map_err(|_| Error::InvalidCertificate(rustls::CertificateError::BadEncoding))?;
    // Pinned by device ID: the 128-bit SHA-256 prefix of the key (forging one takes ~2^128 work).
    // The engine still checks the full public key against the trust store after the handshake.
    if let Some(expected) = pinned {
        if Fingerprint::of_public_key(&key).device_id() != expected {
            return Err(Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
    }
    Ok(())
}

/// Client-side verifier of the server certificate.
#[derive(Debug)]
pub struct PeerServerVerifier {
    pinned: Option<DeviceId>,
    /// This device's own id, and a flag raised when the server turns out to be this device.
    ///
    /// An address can outlive the device that had it: after a DHCP reshuffle the address saved
    /// for a phone can be one this computer now answers on, and dialling it reaches our own
    /// listener. The handshake then fails on the pinned key like any other wrong device, which
    /// tells the caller nothing, so it keeps trying on every reconnect. Recognising our own
    /// certificate lets the caller drop that address instead.
    own: DeviceId,
    dialed_self: Arc<AtomicBool>,
    algs: WebPkiSupportedAlgorithms,
}

impl PeerServerVerifier {
    pub fn new(
        provider: &CryptoProvider,
        pinned: Option<DeviceId>,
        own: DeviceId,
        dialed_self: Arc<AtomicBool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pinned,
            own,
            dialed_self,
            algs: algorithms(provider),
        })
    }
}

impl ServerCertVerifier for PeerServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if device_id_of(end_entity).is_some_and(|id| id == self.own) {
            self.dialed_self.store(true, Ordering::Relaxed);
        }
        check_cert(end_entity, self.pinned)?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// Server-side verifier of client certificates (mutual TLS is mandatory).
#[derive(Debug)]
pub struct PeerClientVerifier {
    algs: WebPkiSupportedAlgorithms,
}

impl PeerClientVerifier {
    pub fn new(provider: &CryptoProvider) -> Arc<Self> {
        Arc::new(Self {
            algs: algorithms(provider),
        })
    }
}

impl ClientCertVerifier for PeerClientVerifier {
    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        check_cert(end_entity, None)?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}
