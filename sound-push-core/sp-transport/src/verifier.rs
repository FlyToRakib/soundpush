//! Certificate verifiers.
//!
//! There is no PKI: each device's certificate is self-signed with its identity
//! key. The verifiers therefore check that the certificate is a well-formed
//! Ed25519 certificate (and, for the client, optionally that it matches a pinned
//! fingerprint) and delegate handshake signature checks to rustls. Trust
//! decisions happen after the handshake in the engine.

use std::sync::Arc;

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
    algs: WebPkiSupportedAlgorithms,
}

impl PeerServerVerifier {
    pub fn new(provider: &CryptoProvider, pinned: Option<DeviceId>) -> Arc<Self> {
        Arc::new(Self {
            pinned,
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
