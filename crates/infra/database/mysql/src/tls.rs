//! TLS for a connection, as its `sslmode` says (libpq's six modes, which the data sources
//! share: see [`database::SslMode`]; MySQL's own `ssl-mode` has the same steps — `DISABLED`,
//! `PREFERRED`, `REQUIRED`, `VERIFY_CA`, `VERIFY_IDENTITY`): which attempts are made
//! ([`attempts`]: plain, encrypted, in order) and how the server's certificate is checked
//! ([`ssl_opts`]: not at all, its chain only, or its chain and its name), against the
//! certificate authority's file when one is named (`sslrootcert`, PEM: then only it is
//! trusted), else Mozilla's roots; a client certificate and its key (`sslcert`, `sslkey`)
//! sent when both are named. rustls does the work (through `mysql_async`).

use std::path::Path;

use database::tls::{SSLCERT, SSLKEY, SSLROOTCERT, Verification};
use database::{DbError, SslMode, Target};
use mysql_async::{ClientIdentity, SslOpts};

/// One way a connection is tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// Without TLS.
    Plain,
    /// With TLS; a server without it is connected to plainly (`prefer`).
    TlsOrPlain(Verification),
    /// With TLS only.
    Tls(Verification),
}

/// The attempts `mode` makes, in order (the next only if the one before is refused for its
/// encryption), given whether a certificate authority's file is named.
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

/// A file read now (so a missing one is said before connecting, by its role).
fn read(path: &Path, what: &str) -> Result<Vec<u8>, DbError> {
    std::fs::read(path).map_err(|e| {
        DbError::Connect(format!("the {what} `{}` can't be read: {e}", path.display()))
    })
}

/// The TLS options checking the server's certificate as `check` says, for `target`.
pub fn ssl_opts(target: &Target, check: Verification) -> Result<SslOpts, DbError> {
    let mut opts = SslOpts::default();
    if let Some(root) = file_option(target, SSLROOTCERT) {
        let pem = read(root, "certificate authority")?;
        opts = opts.with_root_certs(vec![pem.into()]).with_disable_built_in_roots(true);
    }
    opts = match check {
        Verification::None => {
            opts.with_danger_accept_invalid_certs(true).with_danger_skip_domain_validation(true)
        }
        Verification::Chain => opts.with_danger_skip_domain_validation(true),
        Verification::Full => opts,
    };
    match (file_option(target, SSLCERT), file_option(target, SSLKEY)) {
        (Some(cert), Some(key)) => {
            let cert = read(cert, "client certificate")?;
            let key = read(key, "client key")?;
            Ok(opts.with_client_identity(Some(ClientIdentity::new(cert.into(), key.into()))))
        }
        (None, None) => Ok(opts),
        _ => Err(DbError::Connect(
            "a client certificate needs both its file and its key's".to_owned(),
        )),
    }
}

#[cfg(test)]
#[path = "../tests/unit/tls.rs"]
mod tests;
