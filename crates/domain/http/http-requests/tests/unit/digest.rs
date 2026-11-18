//! Digest: RFC 2617's example answered as the RFC says, a challenge read, the short form.

use super::*;

#[test]
fn rfc_2617_s_example_is_answered_as_it_says() {
    let header = r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
    let ch = challenge(header).unwrap();
    assert!(ch.auth_qop && !ch.session && ch.algorithm == Algorithm::Md5);
    let answer =
        authorization(&ch, ("Mufasa", "Circle Of Life"), ("GET", "/dir/index.html"), "0a4f113b", 1);
    assert!(answer.contains(r#"response="6629fae49393a05397450978507c4ef1""#), "{answer}");
    assert!(
        answer.contains("nc=00000001")
            && answer.contains(r#"opaque="5ccc069c403ebaf9f0171e9517f40e41""#)
    );
}

#[test]
fn what_isn_t_a_digest_challenge_or_short_form_is_left() {
    assert_eq!(challenge("Basic realm=\"x\""), None);
    assert_eq!(
        challenge("Digest realm=\"x\", nonce=\"n\", algorithm=SHA-512-256"),
        None,
        "unknown"
    );
    let sha =
        challenge("Digest realm=\"r\", nonce=\"n\", algorithm=SHA-256-sess, qop=auth").unwrap();
    assert!(sha.session && sha.algorithm == Algorithm::Sha256);
    assert_eq!(credentials("Digest ana p4ss word"), Some(("ana".into(), "p4ss word".into())));
    assert_eq!(credentials("Digest username=\"a\", response=\"b\""), None);
}

#[test]
fn a_password_with_an_equals_sign_is_still_the_short_form() {
    assert_eq!(credentials("Digest ana p=ss"), Some(("ana".into(), "p=ss".into())));
    assert_eq!(credentials("Digest username=\"ana\", realm=\"r\""), None);
}
