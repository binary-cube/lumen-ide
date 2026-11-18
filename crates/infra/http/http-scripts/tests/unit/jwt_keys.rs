//! JWT signed with keys: each algorithm signs and checks, a signature made outside (openssl)
//! is taken, a wrong key or a changed text isn't. The keys in `data/` are made for these
//! tests only.

use super::*;

fn data(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/unit/data").join(name),
    )
    .unwrap()
}

/// A PEM file's DER (its base64 between the lines).
fn der(name: &str) -> Vec<u8> {
    let text = String::from_utf8(data(name)).unwrap();
    let body: String = text.lines().filter(|l| !l.starts_with("-----")).collect();
    base64_decode(&body)
}

fn base64_decode(text: &str) -> Vec<u8> {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for c in text.bytes().filter(|c| *c != b'=') {
        let Some(v) = ABC.iter().position(|a| *a == c) else { continue };
        bits = (bits << 6) | v as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out
}

#[test]
fn each_algorithm_signs_and_checks() {
    let text = b"header.payload";
    for (algorithm, key, public) in [
        ("RS256", "rsa.key", "rsa.pub"),
        ("RS512", "rsa.key", "rsa.pub"),
        ("PS256", "rsa.key", "rsa.pub"),
        ("ES256", "ec.key", "ec.pub"),
    ] {
        let signed = sign(algorithm, &der(key), text).unwrap();
        assert_eq!(verify(algorithm, &der(public), text, &signed), Ok(true), "{algorithm}");
        assert_eq!(verify(algorithm, &der(public), b"header.other", &signed), Ok(false));
    }
    assert_eq!(sign("ES256", &der("ec.key"), text).unwrap().len(), 64, "r ‖ s");
    // The other key: not its signature.
    let signed = sign("RS256", &der("rsa.key"), text).unwrap();
    assert_eq!(verify("RS256", &der("ec.pub"), text, &signed), Ok(false));
}

#[test]
fn a_signature_made_outside_is_taken() {
    let (text, signed) = (data("signed.txt"), data("signed.rs256"));
    assert_eq!(verify("RS256", &der("rsa.pub"), &text, &signed), Ok(true));
}

#[test]
fn what_it_can_t_do_is_said() {
    assert!(sign("ES512", &der("ec.key"), b"x").is_err(), "P-521 isn't in ring");
    assert!(sign("RS256", b"not a key", b"x").is_err());
    assert!(sign("ES256", &der("rsa.key"), b"x").is_err(), "an RSA key for ECDSA");
}
