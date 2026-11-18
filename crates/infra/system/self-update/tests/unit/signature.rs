use super::*;
use ed25519_dalek::{Signer, SigningKey};

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A signature checks with its key, over its bytes only; anything else fails.
#[test]
fn a_signature_checks_with_its_key_over_its_bytes() {
    let key = SigningKey::from_bytes(&[7u8; 32]);
    let public = hex(key.verifying_key().as_bytes());
    let signature = hex(&key.sign(b"manifest").to_bytes());
    assert!(verify(b"manifest", &signature, &format!(" {public}\n")));
    assert!(!verify(b"manifest!", &signature, &public), "other bytes");
    let other = hex(SigningKey::from_bytes(&[8u8; 32]).verifying_key().as_bytes());
    assert!(!verify(b"manifest", &signature, &other), "another key");
    assert!(!verify(b"manifest", "00", &public) && !verify(b"manifest", &signature, "zz"));
}

/// The project's key built in is a key.
#[test]
fn the_project_s_key_is_one() {
    assert!(unhex::<32>(crate::PUBLIC_KEY).is_some_and(|k| VerifyingKey::from_bytes(&k).is_ok()));
}
