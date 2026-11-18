use super::*;

#[test]
fn modes_are_read_by_name_and_from_tls() {
    for mode in SslMode::ALL {
        assert_eq!(SslMode::parse(mode.as_str()), Some(mode));
    }
    assert_eq!(SslMode::parse(" VERIFY_FULL "), Some(SslMode::VerifyFull));
    assert_eq!(SslMode::parse("sometimes"), None);
    let mut target = Target::default();
    assert_eq!(SslMode::of(&target), Ok(SslMode::Prefer), "prefer by default");
    target.tls = Tls::Verify;
    assert_eq!(SslMode::of(&target), Ok(SslMode::VerifyFull));
    target.tls = Tls::Trust;
    assert_eq!(SslMode::of(&target), Ok(SslMode::Require));
    target.options.insert(SSLMODE.into(), "disable".into());
    assert_eq!(SslMode::of(&target), Ok(SslMode::Disable), "the option wins");
    target.options.insert(SSLMODE.into(), "nope".into());
    assert!(SslMode::of(&target).is_err(), "never silently weaker");
}

#[test]
fn what_each_mode_does() {
    use Verification::*;
    assert!(!SslMode::Disable.may_encrypt());
    assert!(SslMode::Disable.plain_first() && SslMode::Allow.plain_first());
    assert!(!SslMode::Prefer.plain_first() && !SslMode::Prefer.requires_tls());
    assert!(SslMode::Require.requires_tls() && SslMode::VerifyCa.requires_tls());
    assert_eq!(SslMode::Prefer.verification(true), None);
    assert_eq!(SslMode::Require.verification(false), None);
    assert_eq!(SslMode::Require.verification(true), Chain, "as libpq, with a root file");
    assert_eq!(SslMode::VerifyCa.verification(false), Chain);
    assert_eq!(SslMode::VerifyFull.verification(false), Full);
}
