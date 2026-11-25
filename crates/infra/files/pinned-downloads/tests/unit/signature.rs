use ring::signature::{Ed25519KeyPair, KeyPair};

use super::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A signature checks with its key, over its bytes only; anything else fails.
#[test]
fn a_signature_checks_with_its_key_over_its_bytes() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).unwrap();
    let public = hex(key.public_key().as_ref());
    let signature = hex(key.sign(b"manifest").as_ref());
    assert!(verify(b"manifest", &signature, &format!(" {public}\n")));
    assert!(!verify(b"manifest!", &signature, &public), "other bytes");
    let other = Ed25519KeyPair::from_seed_unchecked(&[8u8; 32]).unwrap();
    assert!(!verify(b"manifest", &signature, &hex(other.public_key().as_ref())), "another key");
    assert!(!verify(b"manifest", "00", &public) && !verify(b"manifest", &signature, "zz"));
}

/// The project's key built in is a key.
#[test]
fn the_projects_key_is_one() {
    assert!(unhex::<32>(PUBLIC_KEY).is_some());
}

/// Only hex digits are read: a sign a number's parse would take is refused.
#[test]
fn only_hex_digits_are_read() {
    assert_eq!(unhex::<2>("0aFf"), Some([0x0a, 0xff]));
    for other in ["+a0f", "0a+f", "0x0f", "0a f", "0a\u{e9}"] {
        assert_eq!(unhex::<2>(other), None, "{other}");
    }
}
