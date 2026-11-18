//! JSON Web Tokens signed with keys (RS256/384/512, PS256/384/512, ES256/384), for the
//! scripts' `jwt.sign` / `jwt.verify`: the keys come as DER (the prelude reads their PEM),
//! signed and checked with `ring` (the same the requests' TLS uses). A private key is
//! PKCS#8 (`PRIVATE KEY`) or, for RSA, PKCS#1 (`RSA PRIVATE KEY`); a public one is
//! `PUBLIC KEY` (SubjectPublicKeyInfo) or, for RSA, `RSA PUBLIC KEY`. ES256 / ES384
//! signatures are `r ‖ s`, as JWT writes them. ES512 (P-521) isn't in `ring`: refused.

use ring::{rand, signature};

/// What an algorithm signs with.
enum Kind {
    Rsa(&'static dyn signature::RsaEncoding, &'static signature::RsaParameters),
    Ec(&'static signature::EcdsaSigningAlgorithm, &'static signature::EcdsaVerificationAlgorithm),
}

fn kind(algorithm: &str) -> Result<Kind, String> {
    use signature as s;
    Ok(match algorithm {
        "RS256" => Kind::Rsa(&s::RSA_PKCS1_SHA256, &s::RSA_PKCS1_2048_8192_SHA256),
        "RS384" => Kind::Rsa(&s::RSA_PKCS1_SHA384, &s::RSA_PKCS1_2048_8192_SHA384),
        "RS512" => Kind::Rsa(&s::RSA_PKCS1_SHA512, &s::RSA_PKCS1_2048_8192_SHA512),
        "PS256" => Kind::Rsa(&s::RSA_PSS_SHA256, &s::RSA_PSS_2048_8192_SHA256),
        "PS384" => Kind::Rsa(&s::RSA_PSS_SHA384, &s::RSA_PSS_2048_8192_SHA384),
        "PS512" => Kind::Rsa(&s::RSA_PSS_SHA512, &s::RSA_PSS_2048_8192_SHA512),
        "ES256" => Kind::Ec(&s::ECDSA_P256_SHA256_FIXED_SIGNING, &s::ECDSA_P256_SHA256_FIXED),
        "ES384" => Kind::Ec(&s::ECDSA_P384_SHA384_FIXED_SIGNING, &s::ECDSA_P384_SHA384_FIXED),
        other => return Err(format!("{other} isn't supported here")),
    })
}

/// `data` signed by private key `key` (DER) with `algorithm`.
pub fn sign(algorithm: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    let random = rand::SystemRandom::new();
    let bad = |e: ring::error::KeyRejected| format!("the key: {e}");
    match kind(algorithm)? {
        Kind::Rsa(encoding, _) => {
            let pair = signature::RsaKeyPair::from_pkcs8(key)
                .or_else(|_| signature::RsaKeyPair::from_der(key))
                .map_err(bad)?;
            let mut out = vec![0; pair.public().modulus_len()];
            pair.sign(encoding, &random, data, &mut out)
                .map_err(|_| "signing failed".to_owned())?;
            Ok(out)
        }
        Kind::Ec(signing, _) => {
            let pair = signature::EcdsaKeyPair::from_pkcs8(signing, key, &random).map_err(bad)?;
            let signed = pair.sign(&random, data).map_err(|_| "signing failed".to_owned())?;
            Ok(signed.as_ref().to_vec())
        }
    }
}

/// Whether `sig` is `data` signed with `algorithm` by the private key of public key `key`
/// (DER: SubjectPublicKeyInfo, or PKCS#1 for RSA).
pub fn verify(algorithm: &str, key: &[u8], data: &[u8], sig: &[u8]) -> Result<bool, String> {
    let raw = subject_public_key(key).unwrap_or(key);
    let checked = match kind(algorithm)? {
        Kind::Rsa(_, parameters) => {
            signature::UnparsedPublicKey::new(parameters, raw).verify(data, sig)
        }
        Kind::Ec(_, verifying) => {
            signature::UnparsedPublicKey::new(verifying, raw).verify(data, sig)
        }
    };
    Ok(checked.is_ok())
}

/// The key inside a SubjectPublicKeyInfo (`SEQUENCE { SEQUENCE { algorithm }, BIT STRING
/// key }`): for RSA its PKCS#1 key, for EC its point. `None`: not one (PKCS#1 already).
fn subject_public_key(der: &[u8]) -> Option<&[u8]> {
    let (tag, outer, _) = element(der)?;
    if tag != 0x30 {
        return None;
    }
    let (tag, _, rest) = element(outer)?;
    if tag != 0x30 {
        return None;
    }
    let (tag, bits, _) = element(rest)?;
    // A bit string: its first byte is how many bits of the last are unused (none here).
    (tag == 0x03 && bits.first() == Some(&0)).then(|| &bits[1..])
}

/// A DER element at the start of `der`: its tag, its content, what follows it.
fn element(der: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = der.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let length = rest[..count].iter().fold(0usize, |n, b| (n << 8) | usize::from(*b));
        (length, &rest[count..])
    };
    (rest.len() >= length).then(|| (tag, &rest[..length], &rest[length..]))
}

#[cfg(test)]
#[path = "../tests/unit/jwt_keys.rs"]
mod tests;
