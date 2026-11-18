//! Which attempts each `sslmode` makes and how the certificate is checked; a client
//! certificate needs both files, and unreadable or non-PEM files are said (no connection
//! made weaker in their place).

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
    let path = std::env::temp_dir().join(format!("pg-tls-{name}-{}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn rustls_is_set_up_for_each_check() {
    let target = Target::default();
    for check in [Unchecked, Chain, Full] {
        assert!(client_config(&target, check).is_ok(), "{check:?} with Mozilla's roots");
    }
}

#[test]
fn bad_certificate_files_are_said() {
    let mut target = Target::default();
    target.options.insert(SSLROOTCERT.into(), "/no/such/ca.pem".into());
    let missing = client_config(&target, Full).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(missing.contains("can't be read"), "{missing}");
    let junk = temp("junk", "not a certificate");
    target.options.insert(SSLROOTCERT.into(), junk.to_string_lossy().into_owned());
    let junk_error = client_config(&target, Chain).err().map(|e| e.to_string());
    assert!(junk_error.unwrap_or_default().contains("no certificate"));
    // A certificate without its key: refused, never sent half.
    let mut half = Target::default();
    half.options.insert(SSLCERT.into(), junk.to_string_lossy().into_owned());
    assert!(
        matches!(client_config(&half, Unchecked), Err(DbError::Connect(m)) if m.contains("both"))
    );
    let _ = std::fs::remove_file(junk);
}
