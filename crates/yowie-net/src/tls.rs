//! TLS configuration for the TCP server and client, using rustls with the
//! ring crypto provider.

use std::path::Path;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, ServerConfig,
    SignatureScheme,
};

use crate::error::{Error, Result};

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn pem_certs(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>> {
    CertificateDer::pem_slice_iter(pem)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Pem(e.to_string()))
        .and_then(|v| {
            if v.is_empty() {
                Err(Error::Pem("no certificate found".into()))
            } else {
                Ok(v)
            }
        })
}

/// Server-side TLS settings. Pass to `TcpServer::listen_tls`.
#[derive(Clone)]
pub struct TlsServer {
    pub(crate) config: Arc<ServerConfig>,
}

impl TlsServer {
    /// Builds settings from a PEM certificate chain and a PEM private key
    /// (PKCS#8, PKCS#1 or SEC1).
    pub fn from_pem(cert_pem: &[u8], key_pem: &[u8]) -> Result<Self> {
        let certs = pem_certs(cert_pem)?;
        let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|e| Error::Pem(e.to_string()))?;
        let config = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
        Ok(Self {
            config: Arc::new(config),
        })
    }

    /// Like [`TlsServer::from_pem`], reading both from files.
    pub fn from_pem_files(cert: impl AsRef<Path>, key: impl AsRef<Path>) -> Result<Self> {
        Self::from_pem(&std::fs::read(cert)?, &std::fs::read(key)?)
    }
}

/// Client-side TLS settings. Pass to `TcpClient::connect_tls`.
#[derive(Clone)]
pub struct TlsClient {
    pub(crate) config: Arc<ClientConfig>,
    pub(crate) name: ServerName<'static>,
}

fn server_name(name: &str) -> Result<ServerName<'static>> {
    ServerName::try_from(name.to_owned()).map_err(|e| Error::Pem(format!("server name: {e}")))
}

impl TlsClient {
    /// Verifies the server against the operating system's trusted roots.
    /// `name` is the host name or IP address the certificate must match.
    pub fn system_roots(name: &str) -> Result<Self> {
        let mut roots = RootCertStore::empty();
        let loaded = rustls_native_certs::load_native_certs();
        for err in &loaded.errors {
            log::debug!("native certificate store: {err}");
        }
        let (added, _) = roots.add_parsable_certificates(loaded.certs);
        if added == 0 {
            return Err(Error::Pem("no usable system root certificates".into()));
        }
        Self::with_roots(name, roots)
    }

    /// Verifies the server against the certificates in `ca_pem` only.
    pub fn with_roots_pem(name: &str, ca_pem: &[u8]) -> Result<Self> {
        let mut roots = RootCertStore::empty();
        for cert in pem_certs(ca_pem)? {
            roots.add(cert)?;
        }
        Self::with_roots(name, roots)
    }

    fn with_roots(name: &str, roots: RootCertStore) -> Result<Self> {
        let config = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            config: Arc::new(config),
            name: server_name(name)?,
        })
    }

    /// Accepts only the server whose end-entity certificate equals the first
    /// certificate in `cert_pem`. Name and expiry are not checked; the
    /// handshake signature still is. `name` is sent as the SNI value.
    pub fn pinned_pem(name: &str, cert_pem: &[u8]) -> Result<Self> {
        let cert = pem_certs(cert_pem)?.remove(0);
        Self::pinned_der(name, cert.as_ref().to_vec())
    }

    /// Like [`TlsClient::pinned_pem`], taking the DER certificate bytes.
    pub fn pinned_der(name: &str, cert_der: Vec<u8>) -> Result<Self> {
        let p = provider();
        let verifier = Arc::new(Pinned {
            cert: cert_der,
            algs: p.signature_verification_algorithms,
        });
        let config = ClientConfig::builder_with_provider(p)
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        Ok(Self {
            config: Arc::new(config),
            name: server_name(name)?,
        })
    }
}

#[derive(Debug)]
struct Pinned {
    cert: Vec<u8>,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.cert.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}
