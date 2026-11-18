//! The hashes and HMACs against known values.

use super::*;

#[test]
fn hashes_and_hmacs_give_the_known_values() {
    assert_eq!(hex(&digest(Algorithm::Md5, b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(hex(&digest(Algorithm::Sha1, b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        hex(&digest(Algorithm::Sha256, b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&hmac(Algorithm::Sha256, b"key", b"The quick brown fox jumps over the lazy dog")),
        "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
    );
    assert_eq!(Algorithm::named("SHA-256"), Some(Algorithm::Sha256));
    assert_eq!(base64(&[0xfb, 0xff], true), "-_8");
    assert_eq!(
        hex(&digest(Algorithm::Sha3_256, b"abc")),
        "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
    );
    assert_eq!(from_hex("00ff10"), Some(vec![0, 255, 16]));
    assert_eq!(from_hex("0g"), None);
}
