//! TLS for a connection, as its `sslmode` says (libpq's rules, see [`database::SslMode`]):
//! which attempts are made ([`attempts`]: plain, encrypted, in order) and how rustls checks
//! the server's certificate ([`client_config`]: not at all, its chain only, or its chain and
//! its name), against the certificate authority's file when one is named (`sslrootcert`,
//! PEM), else Mozilla's roots; a client certificate and its key (`sslcert`, `sslkey`) sent
//! when both are named.

use std::path::Path;
use std::sync::Arc;

use database::tls::{SSLCERT, SSLKEY, SSLROOTCERT, Verification};
use database::{DbError, SslMode, Target};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};

/// One way a connection is tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// Without TLS.
    Plain,
    /// With TLS, the server may say no (then plain): `prefer`.
    TlsOrPlain(Verification),
    /// With TLS only.
    Tls(Verification),
}

/// The attempts `mode` makes, in order (the next only if the one before is refused for
/// its encryption), given whether a certificate authority's file is named.
pub fn attempts(mode: SslMode, has_root: bool) -> Vec<Attempt> {
    let check = mode.verification(has_root);
    match mode {
        SslMode::Disable => vec![Attempt::Plain],
        SslMode::Allow => vec![Attempt::Plain, Attempt::Tls(check)],
        SslMode::Prefer => vec![Attempt::TlsOrPlain(check)],
        SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull => vec![Attempt::Tls(check)],
    }
}

/// A file named by option `name` of `target` (empty: none).
pub fn file_option<'a>(target: &'a Target, name: &str) -> Option<&'a Path> {
    target.options.get(name).map(|f| f.trim()).filter(|f| !f.is_empty()).map(Path::new)
}

fn read(path: &Path, what: &str) -> Result<Vec<u8>, DbError> {
    std::fs::read(path).map_err(|e| {
        DbError::Connect(format!("the {what} `{}` can't be read: {e}", path.display()))
    })
}

/// The certificates of PEM `bytes` (at least one).
fn certificates(bytes: &[u8], what: &str) -> Result<Vec<CertificateDer<'static>>, DbError> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(bytes)
        .collect::<Result<_, _>>()
        .map_err(|e| DbError::Connect(format!("the {what} isn't PEM: {e:?}")))?;
    if certs.is_empty() {
        return Err(DbError::Connect(format!("the {what} holds no certificate")));
    }
    Ok(certs)
}

/// The roots trusted: the certificate authority's file, else Mozilla's.
fn roots(target: &Target) -> Result<rustls::RootCertStore, DbError> {
    let Some(file) = file_option(target, SSLROOTCERT) else {
        return Ok(rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() });
    };
    let mut store = rustls::RootCertStore::empty();
    for cert in certificates(&read(file, "certificate authority")?, "certificate authority")? {
        store
            .add(cert)
            .map_err(|e| DbError::Connect(format!("a bad certificate authority: {e}")))?;
    }
    Ok(store)
}

/// rustls for `target`, checking the server's certificate as `check` says.
pub fn client_config(
    target: &Target,
    check: Verification,
) -> Result<rustls::ClientConfig, DbError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| DbError::Connect(e.to_string()))?;
    let builder = match check {
        Verification::None => {
            builder.dangerous().with_custom_certificate_verifier(Arc::new(TrustAny(provider)))
        }
        Verification::Full => builder.with_root_certificates(roots(target)?),
        Verification::Chain => {
            let inner = rustls::client::WebPkiServerVerifier::builder_with_provider(
                Arc::new(roots(target)?),
                provider,
            )
            .build()
            .map_err(|e| DbError::Connect(e.to_string()))?;
            builder.dangerous().with_custom_certificate_verifier(Arc::new(ChainOnly(inner)))
        }
    };
    match (file_option(target, SSLCERT), file_option(target, SSLKEY)) {
        (Some(cert), Some(key)) => {
            let certs = certificates(&read(cert, "client certificate")?, "client certificate")?;
            let key = PrivateKeyDer::from_pem_slice(&read(key, "client key")?)
                .map_err(|e| DbError::Connect(format!("the client key isn't PEM: {e:?}")))?;
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|e| DbError::Connect(format!("the client certificate: {e}")))
        }
        (None, None) => Ok(builder.with_no_client_auth()),
        _ => Err(DbError::Connect(
            "a client certificate needs both its file and its key's".to_owned(),
        )),
    }
}

/// Accepts the server's certificate whatever it is (`prefer`, `require`: encrypted, the
/// server not proven), still checking its signatures.
#[derive(Debug)]
struct TrustAny(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for TrustAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// `verify-ca`: the chain checked against the trusted roots, the name it gives not.
#[derive(Debug)]
struct ChainOnly(Arc<rustls::client::WebPkiServerVerifier>);

impl ServerCertVerifier for ChainOnly {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.0.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
        {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

#[cfg(test)]
#[path = "../tests/unit/tls.rs"]
mod tests;
