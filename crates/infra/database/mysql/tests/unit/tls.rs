//! Which attempts each `sslmode` makes and how the certificate is checked; a client
//! certificate needs both files, and unreadable files are said (no connection made weaker
//! in their place).

use super::*;
use database::tls::Verification::{Chain, Full, None as Unchecked};

#[test]
fn each_mode_tries_what_libpq_does() {
    assert_eq!(attempts(SslMode::Disable, false), vec![Attempt::Plain]);
    assert_eq!(attempts(SslMode::Allow, false), vec![Attempt::Plain, Attempt::Tls(Unchecked)]);
    assert_eq!(attempts(SslMode::Prefer, false), vec![Attempt::TlsOrPlain(Unchecked)]);
    assert_eq!(attempts(SslMode::Require, false), vec![Attempt::Tls(Unchecked)]);
    assert_eq!(attempts(SslMode::Require, true), vec![Attempt::Tls(Chain)]);
    assert_eq!(attempts(SslMode::VerifyCa, false), vec![Attempt::Tls(Chain)]);
    assert_eq!(attempts(SslMode::VerifyFull, false), vec![Attempt::Tls(Full)]);
}

fn temp(name: &str, text: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("mysql-tls-{name}-{}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn each_check_sets_what_it_trusts() {
    let target = Target::default();
    let none = ssl_opts(&target, Unchecked).unwrap();
    assert!(none.accept_invalid_certs() && none.skip_domain_validation());
    let chain = ssl_opts(&target, Chain).unwrap();
    assert!(!chain.accept_invalid_certs() && chain.skip_domain_validation());
    let full = ssl_opts(&target, Full).unwrap();
    assert!(!full.accept_invalid_certs() && !full.skip_domain_validation());
    assert!(!full.disable_built_in_roots(), "Mozilla's roots without a file of its own");
    // A certificate authority's file: only it is trusted.
    let ca = temp("ca", "-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----\n");
    let mut named = Target::default();
    named.options.insert(SSLROOTCERT.into(), ca.to_string_lossy().into_owned());
    let own = ssl_opts(&named, Full).unwrap();
    assert!(own.disable_built_in_roots() && own.root_certs().len() == 1);
    let _ = std::fs::remove_file(ca);
}

#[test]
fn bad_certificate_files_are_said() {
    let mut target = Target::default();
    target.options.insert(SSLROOTCERT.into(), "/no/such/ca.pem".into());
    let missing = ssl_opts(&target, Full).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(missing.contains("can't be read"), "{missing}");
    // A certificate without its key: refused, never sent half.
    let junk = temp("junk", "not a certificate");
    let mut half = Target::default();
    half.options.insert(SSLCERT.into(), junk.to_string_lossy().into_owned());
    assert!(matches!(ssl_opts(&half, Unchecked), Err(DbError::Connect(m)) if m.contains("both")));
    let _ = std::fs::remove_file(junk);
}
